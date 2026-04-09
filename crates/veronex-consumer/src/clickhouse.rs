use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ClickhouseClient {
    client: Client,
    base_url: String,
    db: String,
    user: String,
    password: String,
}

impl ClickhouseClient {
    pub fn new(base_url: String, db: String, user: String, password: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .expect("Failed to build HTTP client"),
            base_url,
            db,
            user,
            password,
        }
    }

    /// Batch INSERT rows into `table` using ClickHouse HTTP JSONEachRow format.
    ///
    /// Idempotent: ClickHouse deduplicates identical blocks by checksum
    /// (`insert_deduplicate=1`, default). Safe to retry on failure.
    pub async fn insert(&self, table: &str, rows: &[Value]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let body: String = rows
            .iter()
            .filter_map(|r| serde_json::to_string(r).ok())
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!(
            "{base}/?database={db}&query=INSERT+INTO+{table}+FORMAT+JSONEachRow\
             &input_format_skip_unknown_fields=1",
            base = self.base_url,
            db = self.db,
            table = table,
        );

        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.user, Some(&self.password))
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .with_context(|| format!("HTTP request failed for table {table}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ClickHouse INSERT into {table} failed: HTTP {status}: {body}");
        }

        tracing::debug!("Inserted {} rows into {}", rows.len(), table);
        Ok(())
    }
}
