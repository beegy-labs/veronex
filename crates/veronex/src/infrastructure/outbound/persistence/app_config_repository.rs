//! PostgreSQL implementation of [`AppConfigRepository`].
//!
//! Mirrors the encrypt-on-write / decrypt-on-read pattern that
//! `provider_registry` uses for `api_key_encrypted`, with one shared master
//! key (`VERONEX_ENCRYPTION_KEY`).
//!
//! The bulk `upsert_many` path runs as a single transaction so a partial
//! wizard submit can never leave the row in a half-configured state (S3
//! endpoint set but secret key missing).

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::application::ports::outbound::app_config_repository::{
    AppConfigEntry, AppConfigRepository, AppConfigUpsert,
};
use crate::domain::services::encryption::{decrypt_or_legacy, encrypt};

const COLS: &str = "key, value_encrypted, is_secret, updated_at, updated_by";

pub struct PostgresAppConfigRepository {
    pool: PgPool,
    master_key: [u8; 32],
}

impl PostgresAppConfigRepository {
    pub fn new(pool: PgPool, master_key: [u8; 32]) -> Self {
        Self { pool, master_key }
    }

    /// Decrypt the stored ciphertext (when `is_secret`) or return the value
    /// verbatim (when not). NULL value rows are returned as `None`.
    fn decode(&self, row: &sqlx::postgres::PgRow) -> Result<AppConfigEntry> {
        use sqlx::Row as _;
        let key: String = row.try_get("key").context("key")?;
        let raw: Option<String> = row.try_get("value_encrypted").context("value_encrypted")?;
        let is_secret: bool = row.try_get("is_secret").context("is_secret")?;
        let value = match (raw, is_secret) {
            (None, _) => None,
            (Some(v), true) => {
                // `decrypt_or_legacy` returns (plaintext, needs_migration);
                // we ignore the migration flag here — bootstrap re-encrypts
                // on the next upsert.
                let (plaintext, _) = decrypt_or_legacy(&v, &self.master_key);
                Some(plaintext)
            }
            (Some(v), false) => Some(v),
        };
        Ok(AppConfigEntry {
            key,
            value,
            is_secret,
            updated_at: row.try_get("updated_at").context("updated_at")?,
            updated_by: row
                .try_get::<Option<Uuid>, _>("updated_by")
                .context("updated_by")?,
        })
    }

    async fn upsert_inner(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entry: &AppConfigUpsert,
        by: Option<Uuid>,
    ) -> Result<()> {
        let stored: Option<String> = if entry.is_secret {
            Some(encrypt(&entry.value, &self.master_key).with_context(|| {
                format!("encrypt app_config[{}]", entry.key)
            })?)
        } else {
            Some(entry.value.clone())
        };
        sqlx::query(
            "INSERT INTO app_config (key, value_encrypted, is_secret, updated_at, updated_by)
             VALUES ($1, $2, $3, now(), $4)
             ON CONFLICT (key) DO UPDATE SET
                value_encrypted = EXCLUDED.value_encrypted,
                is_secret       = EXCLUDED.is_secret,
                updated_at      = now(),
                updated_by      = EXCLUDED.updated_by",
        )
        .bind(&entry.key)
        .bind(stored)
        .bind(entry.is_secret)
        .bind(by)
        .execute(&mut **tx)
        .await
        .with_context(|| format!("upsert app_config[{}]", entry.key))?;
        Ok(())
    }
}

#[async_trait]
impl AppConfigRepository for PostgresAppConfigRepository {
    async fn get(&self, key: &str) -> Result<Option<AppConfigEntry>> {
        let q = format!("SELECT {COLS} FROM app_config WHERE key = $1");
        let row = sqlx::query(&q).bind(key).fetch_optional(&self.pool).await?;
        row.as_ref().map(|r| self.decode(r)).transpose()
    }

    async fn get_many(&self, keys: &[&str]) -> Result<Vec<AppConfigEntry>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let q = format!("SELECT {COLS} FROM app_config WHERE key = ANY($1)");
        // sqlx handles `Vec<&str>` as a TEXT[] bind.
        let owned: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let rows = sqlx::query(&q)
            .bind(&owned)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| self.decode(r)).collect()
    }

    async fn list_all(&self) -> Result<Vec<AppConfigEntry>> {
        let q = format!("SELECT {COLS} FROM app_config ORDER BY key LIMIT 1000");
        let rows = sqlx::query(&q).fetch_all(&self.pool).await?;
        rows.iter().map(|r| self.decode(r)).collect()
    }

    async fn upsert(&self, entry: &AppConfigUpsert, by: Option<Uuid>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.upsert_inner(&mut tx, entry, by).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn upsert_many(
        &self,
        entries: &[AppConfigUpsert],
        by: Option<Uuid>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for entry in entries {
            self.upsert_inner(&mut tx, entry, by).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM app_config WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}

// `_` to silence the unused-import warning when feature flags differ.
#[allow(dead_code)]
fn _unused_datetime_marker() -> DateTime<Utc> {
    Utc::now()
}
