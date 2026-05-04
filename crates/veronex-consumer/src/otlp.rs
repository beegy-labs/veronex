//! OTLP JSON parsing helpers.
//!
//! OTel Collector's `otlp_json` encoding uses camelCase protobuf field names:
//! `resourceLogs`, `scopeLogs`, `logRecords`, `timeUnixNano` (as string!), `asDouble`, etc.

use serde_json::Value;
use std::collections::HashMap;

/// Extract the string representation from an OTLP `AnyValue` JSON node.
/// Handles `stringValue`, `intValue` (string-encoded), and `asDouble`.
pub fn any_value_to_string(v: &Value) -> String {
    if let Some(s) = v.get("stringValue").and_then(Value::as_str) {
        return s.to_owned();
    }
    if let Some(s) = v.get("intValue").and_then(Value::as_str) {
        return s.to_owned();
    }
    if let Some(n) = v.get("asDouble").and_then(Value::as_f64) {
        return n.to_string();
    }
    if let Some(b) = v.get("boolValue").and_then(Value::as_bool) {
        return b.to_string();
    }
    String::new()
}

/// Convert an OTLP attributes array to `HashMap<String, String>`.
pub fn attrs_to_map(attrs: Option<&Value>) -> HashMap<String, String> {
    let Some(arr) = attrs.and_then(Value::as_array) else {
        return HashMap::new();
    };
    arr.iter()
        .filter_map(|item| {
            let key = item.get("key")?.as_str()?.to_owned();
            let val = item.get("value").map(any_value_to_string).unwrap_or_default();
            Some((key, val))
        })
        .collect()
}

/// Parse a nanosecond Unix timestamp string to u64 nanoseconds.
/// Use for ClickHouse DateTime64(9) fields.
pub fn nano_str_to_ns(s: &str) -> u64 {
    s.parse().unwrap_or(0)
}

/// Parse a nanosecond Unix timestamp string to u64 milliseconds.
/// Use for ClickHouse DateTime64(3) fields.
pub fn nano_str_to_ms(s: &str) -> u64 {
    s.parse::<u64>().unwrap_or(0) / 1_000_000
}
