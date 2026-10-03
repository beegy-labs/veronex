//! PostgreSQL implementation of [`ModelfileRegistry`].
//!
//! The non-trivial pieces are:
//!
//! - `swap_blob` — runs the (UPDATE veronex_models, INSERT/UPDATE new blob,
//!   DECREMENT old blob) trio inside one transaction. Used by the install
//!   orchestrator after a PATCH-triggered re-install completes successfully.
//!
//! - `promote_default` — clears `is_default` on every other row in the
//!   family before setting the target row, so the partial unique index
//!   doesn't reject the UPDATE.
//!
//! - `delete` — also runs in a transaction so the FK reference is removed
//!   before (not after) the blob ref_count is decremented.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::application::ports::outbound::modelfile_registry::{
    ListFilter, ModelfilePatch, ModelfileRegistry,
};
use crate::domain::entities::{ErrorKind, InstallStatus, VeronexModel};

/// Column list for SELECT queries on `veronex_models`. Columns are listed in
/// the same order as the `VeronexModel` struct so `row_to_model` can read
/// them top to bottom without confusion.
const MODEL_COLS: &str = "model_id, family, quantization, blob_sha256, display_name, \
                          source_spec, runtime, defaults, chat_template, stop_tokens, \
                          system_prompt, is_default, install_status, last_error_kind, \
                          last_error_message, last_attempt_at, tags, created_at, updated_at";

pub struct PostgresModelfileRegistry {
    pool: PgPool,
}

impl PostgresModelfileRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_model(row: &sqlx::postgres::PgRow) -> Result<VeronexModel> {
    use sqlx::Row as _;
    let install_status: String =
        row.try_get("install_status").context("install_status")?;
    Ok(VeronexModel {
        model_id: row.try_get("model_id").context("model_id")?,
        family: row.try_get("family").context("family")?,
        quantization: row.try_get("quantization").context("quantization")?,
        blob_sha256: row.try_get("blob_sha256").context("blob_sha256")?,
        display_name: row
            .try_get::<Option<String>, _>("display_name")
            .context("display_name")?,
        source_spec: row
            .try_get::<serde_json::Value, _>("source_spec")
            .context("source_spec")?,
        runtime: row.try_get::<serde_json::Value, _>("runtime").context("runtime")?,
        defaults: row.try_get::<serde_json::Value, _>("defaults").context("defaults")?,
        chat_template: row
            .try_get::<serde_json::Value, _>("chat_template")
            .context("chat_template")?,
        stop_tokens: row
            .try_get::<Option<serde_json::Value>, _>("stop_tokens")
            .context("stop_tokens")?,
        system_prompt: row
            .try_get::<Option<String>, _>("system_prompt")
            .context("system_prompt")?,
        is_default: row.try_get("is_default").context("is_default")?,
        install_status: install_status
            .parse()
            .map_err(|e: String| anyhow::anyhow!("install_status: {e}"))?,
        last_error_kind: row
            .try_get::<Option<String>, _>("last_error_kind")
            .context("last_error_kind")?,
        last_error_message: row
            .try_get::<Option<String>, _>("last_error_message")
            .context("last_error_message")?,
        last_attempt_at: row
            .try_get::<Option<DateTime<Utc>>, _>("last_attempt_at")
            .context("last_attempt_at")?,
        tags: row.try_get::<Option<Vec<String>>, _>("tags").context("tags")?,
        created_at: row.try_get("created_at").context("created_at")?,
        updated_at: row.try_get("updated_at").context("updated_at")?,
    })
}

#[async_trait]
impl ModelfileRegistry for PostgresModelfileRegistry {
    async fn create(&self, m: &VeronexModel) -> Result<()> {
        sqlx::query(
            "INSERT INTO veronex_models
             (model_id, family, quantization, blob_sha256, display_name,
              source_spec, runtime, defaults, chat_template, stop_tokens,
              system_prompt, is_default, install_status,
              last_error_kind, last_error_message, last_attempt_at, tags,
              created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, now(), now())",
        )
        .bind(&m.model_id)
        .bind(&m.family)
        .bind(&m.quantization)
        .bind(&m.blob_sha256)
        .bind(&m.display_name)
        .bind(&m.source_spec)
        .bind(&m.runtime)
        .bind(&m.defaults)
        .bind(&m.chat_template)
        .bind(&m.stop_tokens)
        .bind(&m.system_prompt)
        .bind(m.is_default)
        .bind(m.install_status.as_str())
        .bind(&m.last_error_kind)
        .bind(&m.last_error_message)
        .bind(m.last_attempt_at)
        .bind(&m.tags)
        .execute(&self.pool)
        .await
        .with_context(|| format!("create modelfile {}", m.model_id))?;
        Ok(())
    }

    async fn get(&self, model_id: &str) -> Result<Option<VeronexModel>> {
        let q = format!("SELECT {MODEL_COLS} FROM veronex_models WHERE model_id = $1");
        let row = sqlx::query(&q).bind(model_id).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_model).transpose()
    }

    async fn get_default(&self, family: &str) -> Result<Option<VeronexModel>> {
        // Driven by the partial unique index `uq_veronex_models_family_default`.
        let q = format!(
            "SELECT {MODEL_COLS} FROM veronex_models
             WHERE family = $1 AND is_default = true LIMIT 1"
        );
        let row = sqlx::query(&q).bind(family).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_model).transpose()
    }

    async fn list(&self, filter: &ListFilter) -> Result<Vec<VeronexModel>> {
        let limit = filter.limit.unwrap_or(200);
        let offset = filter.offset.unwrap_or(0);
        // Ternary-style WHERE clauses: `($N IS NULL OR col = $N)` — Postgres
        // optimises these well when the JIT plan includes index lookup.
        let q = format!(
            "SELECT {MODEL_COLS} FROM veronex_models
             WHERE ($1::text IS NULL OR family = $1)
               AND ($2::text IS NULL OR install_status = $2)
               AND ($3::boolean IS NULL OR is_default = $3)
             ORDER BY family, quantization
             LIMIT $4 OFFSET $5"
        );
        let rows = sqlx::query(&q)
            .bind(&filter.family)
            .bind(filter.install_status.map(|s| s.as_str()))
            .bind(filter.is_default)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_model).collect()
    }

    async fn patch(&self, model_id: &str, patch: &ModelfilePatch) -> Result<bool> {
        // Implemented as a single COALESCE-driven UPDATE. Each input field is
        // bound twice: once as a sentinel ("did the caller pass anything?"),
        // once as the new value. When the sentinel is None we keep the
        // existing column value via COALESCE. For nullable fields the
        // double-Option distinguishes "leave alone" from "set NULL"; we
        // marshall them through a sentinel string to preserve that.
        //
        // Build dynamic SET clauses in plain Rust to keep the SQL
        // parameter slot count manageable; this is admin-rate, not request-rate.
        let mut sets: Vec<&'static str> = Vec::new();
        let mut tx = self.pool.begin().await?;

        // Helper to apply each Some(_) field in its own UPDATE round-trip.
        // Trades one SQL round-trip per touched field for readability — admin
        // PATCH is rare so latency doesn't matter.
        if let Some(ref v) = patch.display_name {
            sqlx::query("UPDATE veronex_models SET display_name = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("display_name");
        }
        if let Some(ref v) = patch.source_spec {
            sqlx::query("UPDATE veronex_models SET source_spec = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("source_spec");
        }
        if let Some(ref v) = patch.runtime {
            sqlx::query("UPDATE veronex_models SET runtime = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("runtime");
        }
        if let Some(ref v) = patch.defaults {
            sqlx::query("UPDATE veronex_models SET defaults = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("defaults");
        }
        if let Some(ref v) = patch.chat_template {
            sqlx::query("UPDATE veronex_models SET chat_template = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("chat_template");
        }
        if let Some(ref v) = patch.stop_tokens {
            sqlx::query("UPDATE veronex_models SET stop_tokens = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("stop_tokens");
        }
        if let Some(ref v) = patch.system_prompt {
            sqlx::query("UPDATE veronex_models SET system_prompt = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("system_prompt");
        }
        if let Some(ref v) = patch.tags {
            sqlx::query("UPDATE veronex_models SET tags = $1, updated_at = now() WHERE model_id = $2")
                .bind(v)
                .bind(model_id)
                .execute(&mut *tx)
                .await?;
            sets.push("tags");
        }

        if sets.is_empty() {
            // Nothing to patch — verify the row exists so the handler can 404.
            let exists: Option<(String,)> = sqlx::query_as(
                "SELECT model_id FROM veronex_models WHERE model_id = $1",
            )
            .bind(model_id)
            .fetch_optional(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(exists.is_some());
        }

        // Confirm the row existed before any of the updates.
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT model_id FROM veronex_models WHERE model_id = $1")
                .bind(model_id)
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(exists.is_some())
    }

    async fn update_install_status(
        &self,
        model_id: &str,
        status: InstallStatus,
        error_kind: Option<ErrorKind>,
        error_message: Option<&str>,
        last_attempt_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE veronex_models
             SET install_status = $1,
                 last_error_kind = $2,
                 last_error_message = $3,
                 last_attempt_at = COALESCE($4, last_attempt_at),
                 updated_at = now()
             WHERE model_id = $5",
        )
        .bind(status.as_str())
        .bind(error_kind.map(|e| e.as_str()))
        .bind(error_message)
        .bind(last_attempt_at)
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn swap_blob(
        &self,
        model_id: &str,
        old_sha256: &str,
        new_sha256: &str,
    ) -> Result<()> {
        // The transaction shape mirrors the comment in the trait: bump new
        // ref_count, decrement old, point the model at the new sha256, mark
        // ready. All visible to readers atomically.
        let mut tx: Transaction<'_, Postgres> = self.pool.begin().await?;

        // Increment new (rescues from orphan if applicable).
        sqlx::query(
            "UPDATE gguf_blobs
             SET ref_count = ref_count + 1,
                 orphan_since = NULL,
                 last_accessed_at = now()
             WHERE sha256 = $1",
        )
        .bind(new_sha256)
        .execute(&mut *tx)
        .await?;

        // Repoint model.
        sqlx::query(
            "UPDATE veronex_models
             SET blob_sha256 = $1,
                 install_status = 'ready',
                 last_error_kind = NULL,
                 last_error_message = NULL,
                 updated_at = now()
             WHERE model_id = $2",
        )
        .bind(new_sha256)
        .bind(model_id)
        .execute(&mut *tx)
        .await?;

        // Decrement old; mark orphan_since when ref_count lands at 0.
        sqlx::query(
            "UPDATE gguf_blobs
             SET ref_count = ref_count - 1,
                 orphan_since = CASE
                     WHEN ref_count - 1 = 0 THEN now()
                     ELSE orphan_since
                 END
             WHERE sha256 = $1",
        )
        .bind(old_sha256)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn promote_default(&self, model_id: &str) -> Result<()> {
        // Clearing the existing default and setting the new one in two
        // statements ordered like this avoids tripping the partial unique
        // index `uq_veronex_models_family_default`.
        let mut tx = self.pool.begin().await?;

        // Discover the family of the target row.
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT family FROM veronex_models WHERE model_id = $1 FOR UPDATE",
        )
        .bind(model_id)
        .fetch_optional(&mut *tx)
        .await?;
        let family = match row {
            Some((f,)) => f,
            None => {
                tx.rollback().await?;
                return Err(anyhow::anyhow!("model_id not found: {model_id}"));
            }
        };

        sqlx::query(
            "UPDATE veronex_models
             SET is_default = false, updated_at = now()
             WHERE family = $1 AND model_id <> $2",
        )
        .bind(&family)
        .bind(model_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE veronex_models
             SET is_default = true, updated_at = now()
             WHERE model_id = $1",
        )
        .bind(model_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn delete(&self, model_id: &str) -> Result<bool> {
        // Same shape as decrement_ref but driven by the model's blob_sha256
        // looked up inside the transaction so we don't race with PATCH.
        let mut tx = self.pool.begin().await?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT blob_sha256 FROM veronex_models WHERE model_id = $1 FOR UPDATE",
        )
        .bind(model_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((sha256,)) = row else {
            tx.rollback().await?;
            return Ok(false);
        };

        sqlx::query("DELETE FROM veronex_models WHERE model_id = $1")
            .bind(model_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "UPDATE gguf_blobs
             SET ref_count = ref_count - 1,
                 orphan_since = CASE
                     WHEN ref_count - 1 = 0 THEN now()
                     ELSE orphan_since
                 END
             WHERE sha256 = $1",
        )
        .bind(&sha256)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(true)
    }
}
