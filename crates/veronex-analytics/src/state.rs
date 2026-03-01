use std::sync::Arc;

use crate::otel::OtlpClient;

/// Shared state for the analytics service.
#[derive(Clone)]
pub struct AppState {
    /// ClickHouse client for read queries.
    pub ch: clickhouse::Client,
    /// OTLP HTTP client for emitting inference and audit log records.
    pub otlp: Arc<OtlpClient>,
    /// Bearer token required on all internal endpoints.
    pub analytics_secret: String,
}
