//! otel-traces topic handler.
//!
//! Stores raw OTLP trace payloads as-is in `otel_traces_raw`.
//! No parsing — ClickHouse stores the raw JSON string for future analysis.

use serde_json::{json, Value};

pub fn parse(payload: &[u8]) -> anyhow::Result<Vec<Value>> {
    let payload_str = std::str::from_utf8(payload)
        .map_err(|e| anyhow::anyhow!("Trace payload is not valid UTF-8: {e}"))?
        .to_owned();

    // received_at uses ClickHouse DEFAULT now64(3) — only payload column needed.
    Ok(vec![json!({ "payload": payload_str })])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stores_raw_payload() {
        let raw = br#"{"resourceSpans":[]}"#;
        let rows = parse(raw).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["payload"], r#"{"resourceSpans":[]}"#);
    }

    #[test]
    fn parse_invalid_utf8_returns_error() {
        assert!(parse(&[0xFF, 0xFE]).is_err());
    }
}
