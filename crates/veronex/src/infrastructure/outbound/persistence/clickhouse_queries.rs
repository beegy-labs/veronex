//! SSOT for Clickhouse SQL strings used over the HTTP interface.
//!
//! These run against ClickHouse, not Postgres, but the audit registry
//! flags them too — relocating the strings here keeps handler code
//! free of inline SQL regardless of dialect.

/// 1-minute / 5-minute throughput per audit topic. Caller substitutes
/// `{ch_db}` with the configured database name.
pub fn audit_topic_tpm_sql(ch_db: &str) -> String {
    format!(
        "SELECT 'otel.audit.logs' AS topic, \
                countIf(Timestamp >= now() - INTERVAL 1 MINUTE) AS t1m, \
                countIf(Timestamp >= now() - INTERVAL 5 MINUTE) AS t5m \
         FROM {ch_db}.otel_logs \
         UNION ALL \
         SELECT 'otel.audit.metrics', \
                countIf(ts >= now() - INTERVAL 1 MINUTE), \
                countIf(ts >= now() - INTERVAL 5 MINUTE) \
         FROM {ch_db}.otel_metrics_gauge \
         FORMAT JSONEachRow"
    )
}
