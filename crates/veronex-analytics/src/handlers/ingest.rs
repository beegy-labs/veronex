use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;

// ── Allowed event types (whitelist) ─────────────────────────────────────────

const ALLOWED_EVENT_TYPES: &[&str] = &[
    "inference.completed",
    "audit.action",
    "mcp.tool_call",
];

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

/// Validate an inference ingest request. Returns `Err(StatusCode)` on failure.
fn validate_inference(req: &IngestInferenceRequest) -> Result<(), StatusCode> {
    if req.tenant_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.model_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.provider_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.finish_reason.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.status.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Verify the event type is in the whitelist
    if !ALLOWED_EVENT_TYPES.contains(&"inference.completed") {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

/// `POST /internal/ingest/inference`
///
/// Converts an [`IngestInferenceRequest`] into an OTel LogRecord and emits it
/// via OTLP HTTP -> OTel Collector -> Redpanda [otel-logs] -> ClickHouse.
pub async fn ingest_inference(
    State(state): State<AppState>,
    Json(req): Json<IngestInferenceRequest>,
) -> Result<StatusCode, StatusCode> {
    validate_inference(&req)?;

    let event_time = req.event_time;

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

    state.otlp.emit("inference.completed", event_time, attrs).await;
    Ok(StatusCode::ACCEPTED)
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

/// Validate an audit ingest request. Returns `Err(StatusCode)` on failure.
fn validate_audit(req: &IngestAuditRequest) -> Result<(), StatusCode> {
    if req.account_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.action.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.resource_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.resource_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.resource_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Verify the event type is in the whitelist
    if !ALLOWED_EVENT_TYPES.contains(&"audit.action") {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

/// `POST /internal/ingest/audit`
///
/// Converts an [`IngestAuditRequest`] into an OTel LogRecord and emits it.
pub async fn ingest_audit(
    State(state): State<AppState>,
    Json(req): Json<IngestAuditRequest>,
) -> Result<StatusCode, StatusCode> {
    validate_audit(&req)?;

    let event_time = req.event_time;

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

    state.otlp.emit("audit.action", event_time, attrs).await;
    Ok(StatusCode::ACCEPTED)
}

// ── MCP tool call ingest ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct IngestMcpToolCallRequest {
    pub event_time: DateTime<Utc>,
    pub request_id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub tenant_id: String,
    pub server_id: Uuid,
    pub server_slug: String,
    pub tool_name: String,
    pub namespaced_name: String,
    pub outcome: String,
    pub cache_hit: bool,
    pub latency_ms: u32,
    pub result_bytes: u32,
    pub cap_charged: u8,
    pub loop_round: u8,
}

/// `POST /internal/ingest/mcp`
///
/// Converts an [`IngestMcpToolCallRequest`] into an OTel LogRecord and emits it
/// via OTLP HTTP -> OTel Collector -> Redpanda [otel-logs] -> ClickHouse.
/// The `otel_mcp_tool_calls_mv` materialized view extracts it into `mcp_tool_calls`.
pub async fn ingest_mcp_tool_call(
    State(state): State<AppState>,
    Json(req): Json<IngestMcpToolCallRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut attrs = vec![
        ("event.name",      json!({"stringValue": "mcp.tool_call"})),
        ("request_id",      json!({"stringValue": req.request_id.to_string()})),
        ("tenant_id",       json!({"stringValue": req.tenant_id})),
        ("server_id",       json!({"stringValue": req.server_id.to_string()})),
        ("server_slug",     json!({"stringValue": req.server_slug})),
        ("tool_name",       json!({"stringValue": req.tool_name})),
        ("namespaced_name", json!({"stringValue": req.namespaced_name})),
        ("outcome",         json!({"stringValue": req.outcome})),
        ("cache_hit",       json!({"intValue": (req.cache_hit as u8).to_string()})),
        ("latency_ms",      json!({"intValue": req.latency_ms.to_string()})),
        ("result_bytes",    json!({"intValue": req.result_bytes.to_string()})),
        ("cap_charged",     json!({"intValue": req.cap_charged.to_string()})),
        ("loop_round",      json!({"intValue": req.loop_round.to_string()})),
    ];

    if let Some(id) = req.api_key_id {
        attrs.push(("api_key_id", json!({"stringValue": id.to_string()})));
    }

    state.otlp.emit("mcp.tool_call", req.event_time, attrs).await;
    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    /// Validate that an event name is in the allowed whitelist.
    fn validate_event_name(event_name: &str) -> Result<(), StatusCode> {
        if !ALLOWED_EVENT_TYPES.contains(&event_name) {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(())
    }

    fn make_inference_request() -> IngestInferenceRequest {
        IngestInferenceRequest {
            event_time: Utc::now(),
            request_id: Uuid::new_v4(),
            api_key_id: None,
            tenant_id: "tenant-1".to_string(),
            model_name: "llama3.2".to_string(),
            provider_type: "ollama".to_string(),
            prompt_tokens: 10,
            completion_tokens: 20,
            latency_ms: 150,
            finish_reason: "stop".to_string(),
            status: "success".to_string(),
            error_msg: None,
        }
    }

    fn make_audit_request() -> IngestAuditRequest {
        IngestAuditRequest {
            event_time: Utc::now(),
            account_id: Uuid::new_v4(),
            account_name: "admin".to_string(),
            action: "create".to_string(),
            resource_type: "api_key".to_string(),
            resource_id: Uuid::new_v4().to_string(),
            resource_name: "test-key".to_string(),
            ip_address: Some("127.0.0.1".to_string()),
            details: None,
        }
    }

    // ── Inference validation ────────────────────────────────────────────────

    #[test]
    fn inference_valid_request() {
        assert!(validate_inference(&make_inference_request()).is_ok());
    }

    #[test]
    fn inference_empty_tenant_id() {
        let mut req = make_inference_request();
        req.tenant_id = String::new();
        assert_eq!(validate_inference(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn inference_empty_model_name() {
        let mut req = make_inference_request();
        req.model_name = String::new();
        assert_eq!(validate_inference(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn inference_empty_provider_type() {
        let mut req = make_inference_request();
        req.provider_type = String::new();
        assert_eq!(validate_inference(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn inference_empty_finish_reason() {
        let mut req = make_inference_request();
        req.finish_reason = String::new();
        assert_eq!(validate_inference(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn inference_empty_status() {
        let mut req = make_inference_request();
        req.status = String::new();
        assert_eq!(validate_inference(&req), Err(StatusCode::BAD_REQUEST));
    }

    // ── Audit validation ────────────────────────────────────────────────────

    #[test]
    fn audit_valid_request() {
        assert!(validate_audit(&make_audit_request()).is_ok());
    }

    #[test]
    fn audit_empty_account_name() {
        let mut req = make_audit_request();
        req.account_name = String::new();
        assert_eq!(validate_audit(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn audit_empty_action() {
        let mut req = make_audit_request();
        req.action = String::new();
        assert_eq!(validate_audit(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn audit_empty_resource_type() {
        let mut req = make_audit_request();
        req.resource_type = String::new();
        assert_eq!(validate_audit(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn audit_empty_resource_id() {
        let mut req = make_audit_request();
        req.resource_id = String::new();
        assert_eq!(validate_audit(&req), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn audit_empty_resource_name() {
        let mut req = make_audit_request();
        req.resource_name = String::new();
        assert_eq!(validate_audit(&req), Err(StatusCode::BAD_REQUEST));
    }

    // ── Event name whitelist ────────────────────────────────────────────────

    #[test]
    fn valid_event_names() {
        assert!(validate_event_name("inference.completed").is_ok());
        assert!(validate_event_name("audit.action").is_ok());
    }

    #[test]
    fn unknown_event_name_rejected() {
        assert_eq!(
            validate_event_name("unknown.event"),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn empty_event_name_rejected() {
        assert_eq!(
            validate_event_name(""),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn sql_injection_event_name_rejected() {
        assert_eq!(
            validate_event_name("'; DROP TABLE--"),
            Err(StatusCode::BAD_REQUEST)
        );
    }
}
