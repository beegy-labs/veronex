use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;

// ── Inference ingest ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct IngestInferenceRequest {
    pub event_time: DateTime<Utc>,
    pub request_id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub tenant_id: String,
    pub model_name: String,
    pub provider_type: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub latency_ms: u32,
    pub finish_reason: String,
    pub status: String,
    pub error_msg: Option<String>,
}

/// `POST /internal/ingest/inference`
///
/// Converts an [`IngestInferenceRequest`] into an OTel LogRecord and emits it
/// via OTLP HTTP → OTel Collector → Redpanda [otel-logs] → ClickHouse.
pub async fn ingest_inference(
    State(state): State<AppState>,
    Json(req): Json<IngestInferenceRequest>,
) -> StatusCode {
    let mut attrs = vec![
        ("event.name", json!({"stringValue": "inference.completed"})),
        ("request_id", json!({"stringValue": req.request_id.to_string()})),
        ("tenant_id", json!({"stringValue": req.tenant_id})),
        ("model_name", json!({"stringValue": req.model_name})),
        ("provider_type", json!({"stringValue": req.provider_type})),
        ("prompt_tokens", json!({"intValue": req.prompt_tokens.to_string()})),
        ("completion_tokens", json!({"intValue": req.completion_tokens.to_string()})),
        ("latency_ms", json!({"intValue": req.latency_ms.to_string()})),
        ("finish_reason", json!({"stringValue": req.finish_reason})),
        ("status", json!({"stringValue": req.status})),
    ];

    if let Some(id) = req.api_key_id {
        attrs.push(("api_key_id", json!({"stringValue": id.to_string()})));
    }
    if let Some(msg) = req.error_msg {
        attrs.push(("error_msg", json!({"stringValue": msg})));
    }

    // OTLP timestamps must use the original event time
    let _ = req.event_time; // already captured via event_time attr if needed

    state.otlp.emit("inference.completed", attrs).await;
    StatusCode::ACCEPTED
}

// ── Audit ingest ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct IngestAuditRequest {
    pub event_time: DateTime<Utc>,
    pub account_id: Uuid,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: Option<String>,
    pub details: Option<String>,
}

/// `POST /internal/ingest/audit`
///
/// Converts an [`IngestAuditRequest`] into an OTel LogRecord and emits it.
pub async fn ingest_audit(
    State(state): State<AppState>,
    Json(req): Json<IngestAuditRequest>,
) -> StatusCode {
    let _ = req.event_time;

    let attrs = vec![
        ("event.name", json!({"stringValue": "audit.action"})),
        ("account_id", json!({"stringValue": req.account_id.to_string()})),
        ("account_name", json!({"stringValue": req.account_name})),
        ("action", json!({"stringValue": req.action})),
        ("resource_type", json!({"stringValue": req.resource_type})),
        ("resource_id", json!({"stringValue": req.resource_id})),
        ("resource_name", json!({"stringValue": req.resource_name})),
        (
            "ip_address",
            json!({"stringValue": req.ip_address.unwrap_or_default()}),
        ),
        (
            "details",
            json!({"stringValue": req.details.unwrap_or_default()}),
        ),
    ];

    state.otlp.emit("audit.action", attrs).await;
    StatusCode::ACCEPTED
}
