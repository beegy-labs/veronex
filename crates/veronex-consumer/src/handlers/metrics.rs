/// otel-metrics topic handler.
///
/// Parses an OTLP JSON metrics batch and produces rows for `otel_metrics_gauge`.
/// Handles both gauge and sum (isMonotonic) metric types — agent classifies in scraper.rs.

use serde_json::{json, Value};

use crate::otlp::{attrs_to_map, nano_str_to_secs};

pub fn parse(payload: &[u8]) -> anyhow::Result<Vec<Value>> {
    let root: Value = serde_json::from_slice(payload)?;
    let mut rows = Vec::new();

    let resource_metrics = root
        .get("resourceMetrics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for rm in &resource_metrics {
        let resource = rm.get("resource").unwrap_or(&Value::Null);
        let resource_attrs = attrs_to_map(resource.get("attributes"));
        let server_id = resource_attrs
            .get("server_id")
            .cloned()
            .unwrap_or_default();

        let scope_metrics = rm
            .get("scopeMetrics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for sm in &scope_metrics {
            let metrics = sm
                .get("metrics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for metric in &metrics {
                let metric_name = metric.get("name").and_then(Value::as_str).unwrap_or("");

                // gauge or sum — agent already classified, we handle both identically
                let data_points = metric
                    .get("gauge")
                    .or_else(|| metric.get("sum"))
                    .and_then(|m| m.get("dataPoints"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                for dp in &data_points {
                    let time_ns = dp
                        .get("timeUnixNano")
                        .and_then(Value::as_str)
                        .unwrap_or("0");
                    let ts = nano_str_to_secs(time_ns);
                    let value = dp.get("asDouble").and_then(Value::as_f64).unwrap_or(0.0);
                    let dp_attrs = attrs_to_map(dp.get("attributes"));

                    rows.push(json!({
                        "ts":          ts,
                        "server_id":   server_id,
                        "metric_name": metric_name,
                        "value":       value,
                        "attributes":  dp_attrs,
                    }));
                }
            }
        }
    }

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_gauge_payload(metric_name: &str, server_id: &str, value: f64) -> Vec<u8> {
        let payload = json!({
            "resourceMetrics": [{
                "resource": {
                    "attributes": [
                        {"key": "server_id", "value": {"stringValue": server_id}}
                    ]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": metric_name,
                        "gauge": {
                            "dataPoints": [{
                                "timeUnixNano": "1712345678000000000",
                                "asDouble": value,
                                "attributes": []
                            }]
                        }
                    }]
                }]
            }]
        });
        serde_json::to_vec(&payload).unwrap()
    }

    fn make_sum_payload(metric_name: &str, server_id: &str, value: f64) -> Vec<u8> {
        let payload = json!({
            "resourceMetrics": [{
                "resource": {
                    "attributes": [
                        {"key": "server_id", "value": {"stringValue": server_id}}
                    ]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": metric_name,
                        "sum": {
                            "isMonotonic": true,
                            "dataPoints": [{
                                "timeUnixNano": "1712345678000000000",
                                "asDouble": value,
                                "attributes": [
                                    {"key": "mode", "value": {"stringValue": "idle"}}
                                ]
                            }]
                        }
                    }]
                }]
            }]
        });
        serde_json::to_vec(&payload).unwrap()
    }

    #[test]
    fn parse_gauge_metric() {
        let payload = make_gauge_payload("node_memory_MemTotal_bytes", "srv-1", 16_000_000.0);
        let rows = parse(&payload).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["metric_name"], "node_memory_MemTotal_bytes");
        assert_eq!(rows[0]["server_id"], "srv-1");
        assert_eq!(rows[0]["value"], 16_000_000.0);
    }

    #[test]
    fn parse_sum_metric() {
        let payload = make_sum_payload("node_cpu_seconds_total", "srv-1", 1234.5);
        let rows = parse(&payload).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["metric_name"], "node_cpu_seconds_total");
        assert_eq!(rows[0]["attributes"]["mode"], "idle");
    }

    #[test]
    fn parse_empty_payload_returns_error() {
        assert!(parse(b"").is_err());
    }

    #[test]
    fn parse_empty_resource_metrics_returns_empty() {
        let payload = serde_json::to_vec(&json!({"resourceMetrics": []})).unwrap();
        let rows = parse(&payload).unwrap();
        assert!(rows.is_empty());
    }
}
