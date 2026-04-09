//! otel-logs topic handler.
//!
//! Parses an OTLP JSON log batch and fans out rows to:
//!   - `otel_logs`       — unified raw event store (all events, 7-day TTL)
//!   - `inference_logs`  — event.name = "inference.completed"
//!   - `audit_events`    — event.name = "audit.action"
//!   - `mcp_tool_calls`  — event.name = "mcp.tool_call"
//!
//! Fan-out is done here instead of ClickHouse MVs to get explicit offset
//! commit control: offsets are committed only after ALL INSERTs succeed.

use serde_json::{json, Value};

use crate::otlp::{attrs_to_map, nano_str_to_secs};

#[derive(Default)]
pub struct LogRows {
    pub otel_logs: Vec<Value>,
    pub inference_logs: Vec<Value>,
    pub audit_events: Vec<Value>,
    pub mcp_tool_calls: Vec<Value>,
}

impl LogRows {
    pub fn len(&self) -> usize {
        self.otel_logs.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.otel_logs.is_empty()
    }

    pub fn extend(&mut self, other: LogRows) {
        self.otel_logs.extend(other.otel_logs);
        self.inference_logs.extend(other.inference_logs);
        self.audit_events.extend(other.audit_events);
        self.mcp_tool_calls.extend(other.mcp_tool_calls);
    }

    pub fn clear(&mut self) {
        self.otel_logs.clear();
        self.inference_logs.clear();
        self.audit_events.clear();
        self.mcp_tool_calls.clear();
    }
}

pub fn parse(payload: &[u8]) -> anyhow::Result<LogRows> {
    let root: Value = serde_json::from_slice(payload)?;
    let mut out = LogRows::default();

    let resource_logs = root
        .get("resourceLogs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for rm in &resource_logs {
        let resource = rm.get("resource").unwrap_or(&Value::Null);
        let resource_attrs = attrs_to_map(resource.get("attributes"));
        let service_name = resource_attrs
            .get("service.name")
            .cloned()
            .unwrap_or_default();

        let scope_logs = rm
            .get("scopeLogs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for sl in &scope_logs {
            let log_records = sl
                .get("logRecords")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for lr in &log_records {
                let time_ns = lr
                    .get("timeUnixNano")
                    .and_then(Value::as_str)
                    .unwrap_or("0");
                let timestamp = nano_str_to_secs(time_ns);

                let log_attrs = attrs_to_map(lr.get("attributes"));
                let event_name = log_attrs
                    .get("event.name")
                    .cloned()
                    .unwrap_or_default();

                // Unified raw store (all events, 7-day TTL)
                out.otel_logs.push(json!({
                    "Timestamp":          timestamp,
                    "TraceId":            lr.get("traceId").and_then(Value::as_str).unwrap_or(""),
                    "SpanId":             lr.get("spanId").and_then(Value::as_str).unwrap_or(""),
                    "SeverityText":       lr.get("severityText").and_then(Value::as_str).unwrap_or("INFO"),
                    "SeverityNumber":     lr.get("severityNumber").and_then(Value::as_i64).unwrap_or(9),
                    "ServiceName":        service_name,
                    "Body":               lr.get("body")
                                            .and_then(|b| b.get("stringValue"))
                                            .and_then(Value::as_str)
                                            .unwrap_or(""),
                    "ResourceAttributes": resource_attrs,
                    "LogAttributes":      log_attrs,
                }));

                // Route to specialized table by event.name
                match event_name.as_str() {
                    "inference.completed" => {
                        out.inference_logs.push(json!({
                            "event_time":        timestamp,
                            "api_key_id":        log_attrs.get("api_key_id").cloned().unwrap_or_default(),
                            "tenant_id":         log_attrs.get("tenant_id").cloned().unwrap_or_default(),
                            "request_id":        log_attrs.get("request_id").cloned().unwrap_or_default(),
                            "model_name":        log_attrs.get("model_name").cloned().unwrap_or_default(),
                            "prompt_tokens":     log_attrs.get("prompt_tokens")
                                                     .and_then(|v| v.parse::<u32>().ok())
                                                     .unwrap_or(0),
                            "completion_tokens": log_attrs.get("completion_tokens")
                                                     .and_then(|v| v.parse::<u32>().ok())
                                                     .unwrap_or(0),
                            "latency_ms":        log_attrs.get("latency_ms")
                                                     .and_then(|v| v.parse::<u32>().ok())
                                                     .unwrap_or(0),
                            "finish_reason":     log_attrs.get("finish_reason").cloned().unwrap_or_default(),
                            "status":            log_attrs.get("status").cloned().unwrap_or_default(),
                        }));
                    }
                    "audit.action" => {
                        out.audit_events.push(json!({
                            "event_time":    timestamp,
                            "account_id":    log_attrs.get("account_id").cloned().unwrap_or_default(),
                            "account_name":  log_attrs.get("account_name").cloned().unwrap_or_default(),
                            "action":        log_attrs.get("action").cloned().unwrap_or_default(),
                            "resource_type": log_attrs.get("resource_type").cloned().unwrap_or_default(),
                            "resource_id":   log_attrs.get("resource_id").cloned().unwrap_or_default(),
                            "resource_name": log_attrs.get("resource_name").cloned().unwrap_or_default(),
                            "ip_address":    log_attrs.get("ip_address").cloned().unwrap_or_default(),
                            "details":       log_attrs.get("details").cloned().unwrap_or_default(),
                        }));
                    }
                    "mcp.tool_call" => {
                        out.mcp_tool_calls.push(json!({
                            "event_time":      timestamp,
                            "request_id":      log_attrs.get("request_id").cloned().unwrap_or_default(),
                            "api_key_id":      log_attrs.get("api_key_id").cloned().unwrap_or_default(),
                            "tenant_id":       log_attrs.get("tenant_id").cloned().unwrap_or_default(),
                            "server_id":       log_attrs.get("server_id").cloned().unwrap_or_default(),
                            "server_slug":     log_attrs.get("server_slug").cloned().unwrap_or_default(),
                            "tool_name":       log_attrs.get("tool_name").cloned().unwrap_or_default(),
                            "namespaced_name": log_attrs.get("namespaced_name").cloned().unwrap_or_default(),
                            "outcome":         log_attrs.get("outcome").cloned().unwrap_or_default(),
                            "cache_hit":       log_attrs.get("cache_hit")
                                                   .and_then(|v| v.parse::<u8>().ok())
                                                   .unwrap_or(0),
                            "latency_ms":      log_attrs.get("latency_ms")
                                                   .and_then(|v| v.parse::<u32>().ok())
                                                   .unwrap_or(0),
                            "result_bytes":    log_attrs.get("result_bytes")
                                                   .and_then(|v| v.parse::<u32>().ok())
                                                   .unwrap_or(0),
                            "cap_charged":     log_attrs.get("cap_charged")
                                                   .and_then(|v| v.parse::<u8>().ok())
                                                   .unwrap_or(0),
                            "loop_round":      log_attrs.get("loop_round")
                                                   .and_then(|v| v.parse::<u8>().ok())
                                                   .unwrap_or(0),
                        }));
                    }
                    _ => {
                        tracing::debug!("Unrecognised event.name: {event_name} — stored in otel_logs only");
                    }
                }
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log_payload(event_name: &str, extra_attrs: &[(&str, &str)]) -> Vec<u8> {
        let mut attrs = vec![
            json!({"key": "event.name", "value": {"stringValue": event_name}}),
        ];
        for (k, v) in extra_attrs {
            attrs.push(json!({"key": k, "value": {"stringValue": v}}));
        }
        let payload = json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": "veronex-analytics"}}
                    ]
                },
                "scopeLogs": [{
                    "logRecords": [{
                        "timeUnixNano": "1712345678000000000",
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": event_name},
                        "attributes": attrs
                    }]
                }]
            }]
        });
        serde_json::to_vec(&payload).unwrap()
    }

    #[test]
    fn parse_always_populates_otel_logs() {
        let payload = make_log_payload("inference.completed", &[
            ("request_id", "00000000-0000-0000-0000-000000000001"),
            ("tenant_id", "t1"),
            ("model_name", "llama3"),
            ("provider_type", "ollama"),
            ("finish_reason", "stop"),
            ("status", "success"),
            ("prompt_tokens", "10"),
            ("completion_tokens", "20"),
            ("latency_ms", "150"),
        ]);
        let rows = parse(&payload).unwrap();
        assert_eq!(rows.otel_logs.len(), 1);
        assert_eq!(rows.inference_logs.len(), 1);
        assert!(rows.audit_events.is_empty());
        assert!(rows.mcp_tool_calls.is_empty());
    }

    #[test]
    fn parse_routes_audit_event() {
        let payload = make_log_payload("audit.action", &[
            ("account_id", "00000000-0000-0000-0000-000000000002"),
            ("account_name", "admin"),
            ("action", "create"),
            ("resource_type", "api_key"),
            ("resource_id", "r1"),
            ("resource_name", "test-key"),
        ]);
        let rows = parse(&payload).unwrap();
        assert_eq!(rows.audit_events.len(), 1);
        assert!(rows.inference_logs.is_empty());
    }

    #[test]
    fn parse_unknown_event_name_stored_in_otel_logs_only() {
        let payload = make_log_payload("unknown.event", &[]);
        let rows = parse(&payload).unwrap();
        assert_eq!(rows.otel_logs.len(), 1);
        assert!(rows.inference_logs.is_empty());
        assert!(rows.audit_events.is_empty());
        assert!(rows.mcp_tool_calls.is_empty());
    }

    #[test]
    fn parse_empty_payload_returns_error() {
        assert!(parse(b"").is_err());
    }

    #[test]
    fn parse_empty_resource_logs_returns_empty() {
        let payload = serde_json::to_vec(&json!({"resourceLogs": []})).unwrap();
        let rows = parse(&payload).unwrap();
        assert!(rows.is_empty());
    }
}
