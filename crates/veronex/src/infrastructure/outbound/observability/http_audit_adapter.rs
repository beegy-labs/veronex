use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use crate::application::ports::outbound::audit_port::{AuditEvent, AuditPort};

/// HTTP adapter that forwards audit events to `veronex-analytics`
/// via `POST /internal/ingest/audit`.
///
/// Fail-open: errors are logged as warnings and swallowed.
pub struct HttpAuditAdapter {
    http: reqwest::Client,
    analytics_url: String,
    secret: String,
}

impl HttpAuditAdapter {
    pub fn new(
        client: reqwest::Client,
        analytics_url: impl Into<String>,
        secret: impl Into<String>,
    ) -> Self {
        Self {
            http: client,
            analytics_url: analytics_url.into(),
            secret: secret.into(),
        }
    }
}

#[derive(Serialize)]
struct IngestAuditPayload {
    pub event_time: chrono::DateTime<Utc>,
    pub account_id: uuid::Uuid,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: Option<String>,
    pub details: Option<String>,
}

#[async_trait]
impl AuditPort for HttpAuditAdapter {
    async fn record(&self, event: AuditEvent) {
        let payload = IngestAuditPayload {
            event_time: event.event_time,
            account_id: event.account_id,
            account_name: event.account_name,
            action: event.action,
            resource_type: event.resource_type,
            resource_id: event.resource_id,
            resource_name: event.resource_name,
            ip_address: event.ip_address,
            details: event.details,
        };

        let url = format!("{}/internal/ingest/audit", self.analytics_url);
        match self
            .http
            .post(&url)
            .bearer_auth(&self.secret)
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    tracing::warn!(
                        status = %resp.status(),
                        "analytics ingest audit failed"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "analytics ingest audit transport error"
                );
            }
        }
    }
}
