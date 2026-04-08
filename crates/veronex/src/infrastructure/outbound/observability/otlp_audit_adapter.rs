use async_trait::async_trait;
use serde_json::json;

use crate::application::ports::outbound::audit_port::{AuditEvent, AuditPort};

use super::otlp_client::OtlpClient;

/// OTLP adapter that emits audit events directly to the OTel Collector
/// via `POST /v1/logs` (OTLP HTTP/JSON).
///
/// Replaces the two-hop `veronex → veronex-analytics → OTel Collector` path.
/// Schema matches `veronex-analytics::handlers::ingest::ingest_audit`.
///
/// Fail-open: errors are logged as warnings and swallowed.
pub struct OtlpAuditAdapter {
    otlp: OtlpClient,
}

impl OtlpAuditAdapter {
    pub fn new(otel_http_endpoint: &str) -> Self {
        Self {
            otlp: OtlpClient::new(otel_http_endpoint),
        }
    }
}

#[async_trait]
impl AuditPort for OtlpAuditAdapter {
    async fn record(&self, event: AuditEvent) {
        let attrs = vec![
            ("event.name", json!({"stringValue": "audit.action"})),
            ("account_id", json!({"stringValue": event.account_id.to_string()})),
            ("account_name", json!({"stringValue": event.account_name})),
            ("action", json!({"stringValue": event.action})),
            ("resource_type", json!({"stringValue": event.resource_type})),
            ("resource_id", json!({"stringValue": event.resource_id})),
            ("resource_name", json!({"stringValue": event.resource_name})),
            (
                "ip_address",
                json!({"stringValue": event.ip_address.unwrap_or_default()}),
            ),
            (
                "details",
                json!({"stringValue": event.details.unwrap_or_default()}),
            ),
        ];

        self.otlp
            .emit("audit.action", event.event_time, attrs)
            .await;
    }
}
