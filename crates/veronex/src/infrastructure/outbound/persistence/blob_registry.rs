//! PostgreSQL implementation of [`BlobRegistry`].
//!
//! All ref_count mutations are single SQL statements so concurrent installers
//! converge on a single blob row without losing references. The
//! `insert_or_increment` UPSERT is the linchpin — two replicas that resolve
//! the same sha256 in parallel each successfully end up with `ref_count = 2`
//! (one per Modelfile they're registering) without race-condition leaks.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::application::ports::outbound::blob_registry::{BlobRegistry, OrphanFilter};
use crate::domain::entities::GgufBlob;

/// Column list shared by all SELECT queries on `gguf_blobs`. Centralised
/// so adding a column doesn't fan out to every callsite.
const BLOB_COLS: &str = "sha256, size_bytes, s3_key, source_history, ref_count, \
                         orphan_since, created_at, last_accessed_at";

pub struct PostgresBlobRegistry {
    pool: PgPool,
}

impl PostgresBlobRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_blob(row: &sqlx::postgres::PgRow) -> Result<GgufBlob> {
    use sqlx::Row as _;
    let size_bytes: i64 = row.try_get("size_bytes").context("size_bytes")?;
    Ok(GgufBlob {
        sha256: row.try_get("sha256").context("sha256")?,
        size_bytes: size_bytes.max(0) as u64,
        s3_key: row.try_get("s3_key").context("s3_key")?,
        source_history: row
            .try_get::<serde_json::Value, _>("source_history")
            .context("source_history")?,
        ref_count: row.try_get("ref_count").context("ref_count")?,
        orphan_since: row
            .try_get::<Option<DateTime<Utc>>, _>("orphan_since")
            .context("orphan_since")?,
        created_at: row.try_get("created_at").context("created_at")?,
        last_accessed_at: row
            .try_get::<Option<DateTime<Utc>>, _>("last_accessed_at")
            .context("last_accessed_at")?,
    })
}

#[async_trait]
impl BlobRegistry for PostgresBlobRegistry {
    async fn get(&self, sha256: &str) -> Result<Option<GgufBlob>> {
        let q = format!("SELECT {BLOB_COLS} FROM gguf_blobs WHERE sha256 = $1");
        let row = sqlx::query(&q).bind(sha256).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_blob).transpose()
    }

    async fn insert_or_increment(
        &self,
        sha256: &str,
        size_bytes: u64,
        s3_key: &str,
        source_record: &serde_json::Value,
    ) -> Result<i32> {
        // The UPSERT here is the heart of CAS dedup. ON CONFLICT bumps
        // ref_count, clears orphan_since (a previously orphaned blob is
        // "rescued" by the new registration), and appends to source_history
        // so we can audit which sources produced this hash over time.
        let row: (i32,) = sqlx::query_as(
            "INSERT INTO gguf_blobs
                (sha256, size_bytes, s3_key, source_history,
                 ref_count, orphan_since, created_at, last_accessed_at)
             VALUES ($1, $2, $3, jsonb_build_array($4), 1, NULL, now(), now())
             ON CONFLICT (sha256) DO UPDATE SET
                ref_count = gguf_blobs.ref_count + 1,
                orphan_since = NULL,
                source_history = gguf_blobs.source_history || jsonb_build_array($4),
                last_accessed_at = now()
             RETURNING ref_count",
        )
        .bind(sha256)
        .bind(size_bytes as i64)
        .bind(s3_key)
        .bind(source_record)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn increment_ref(&self, sha256: &str) -> Result<i32> {
        let row: (i32,) = sqlx::query_as(
            "UPDATE gguf_blobs
             SET ref_count = ref_count + 1,
                 orphan_since = NULL,
                 last_accessed_at = now()
             WHERE sha256 = $1
             RETURNING ref_count",
        )
        .bind(sha256)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("increment_ref({sha256})"))?;
        Ok(row.0)
    }

    async fn decrement_ref(&self, sha256: &str) -> Result<i32> {
        // Single-statement decrement + conditional orphan_since update. We
        // CASE on the ref_count *after* decrement: if it lands at 0 set the
        // marker, otherwise keep the prior value (which is NULL for any
        // referenced blob).
        let row: (i32,) = sqlx::query_as(
            "UPDATE gguf_blobs
             SET ref_count = ref_count - 1,
                 orphan_since = CASE
                     WHEN ref_count - 1 = 0 THEN now()
                     ELSE orphan_since
                 END
             WHERE sha256 = $1
             RETURNING ref_count",
        )
        .bind(sha256)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("decrement_ref({sha256})"))?;
        Ok(row.0)
    }

    async fn list_orphans(&self, filter: &OrphanFilter) -> Result<Vec<GgufBlob>> {
        // Driven by `idx_gguf_blobs_orphan` (partial index where ref_count = 0).
        let limit = filter.limit.unwrap_or(200);
        let offset = filter.offset.unwrap_or(0);
        let q = format!(
            "SELECT {BLOB_COLS} FROM gguf_blobs
             WHERE ref_count = 0
               AND ($1::bigint IS NULL OR orphan_since <= now() - make_interval(days => $1::int))
             ORDER BY orphan_since DESC NULLS LAST
             LIMIT $2 OFFSET $3"
        );
        let rows = sqlx::query(&q)
            .bind(filter.min_orphan_days)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_blob).collect()
    }

    async fn touch_accessed(&self, sha256: &str, at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE gguf_blobs SET last_accessed_at = $2 WHERE sha256 = $1")
            .bind(sha256)
            .bind(at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, sha256: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM gguf_blobs WHERE sha256 = $1")
            .bind(sha256)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}
