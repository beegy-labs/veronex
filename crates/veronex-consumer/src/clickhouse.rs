use std::time::Duration;

use reqwest::Client;
use serde_json::Value;

/// Distinguishes retryable (5xx / network) from non-retryable (4xx bad data) errors.
#[derive(Debug)]
pub enum InsertError {
    /// HTTP 4xx — bad data, retrying won't help. Caller should discard the rows.
    BadData(String),
    /// Network error or 5xx — transient, should retry.
    Transient(anyhow::Error),
}

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
    /// Returns `Ok(())` on success, `Err(InsertError::BadData)` on HTTP 4xx
    /// (discard rows — retrying won't help), `Err(InsertError::Transient)` on
    /// network errors or 5xx (caller should retry).
    pub async fn insert(&self, table: &str, rows: &[Value]) -> Result<(), InsertError> {
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
            .map_err(|e| InsertError::Transient(anyhow::anyhow!("HTTP request failed for table {table}: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: String = resp.text().await.unwrap_or_default();
            let msg = format!("ClickHouse INSERT into {table} failed: HTTP {status}: {body}");
            if status.is_client_error() {
                return Err(InsertError::BadData(msg));
            }
            return Err(InsertError::Transient(anyhow::anyhow!(msg)));
        }

        tracing::debug!("Inserted {} rows into {}", rows.len(), table);
        Ok(())
    }
}
