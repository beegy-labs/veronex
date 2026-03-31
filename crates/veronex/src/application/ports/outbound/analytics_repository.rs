use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Aggregate usage ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageAggregate {
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ── Hourly usage ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyUsage {
    pub hour: String,
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ── Performance metrics ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyThroughput {
    pub hour: String,
    pub request_count: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_requests: u64,
    pub success_rate: f64,
    pub total_tokens: u64,
    pub hourly: Vec<HourlyThroughput>,
}

// ── Server metrics history ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsPoint {
    pub ts: String,
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
    pub gpu_temp_junction_c: Option<f64>,
    pub gpu_temp_mem_c: Option<f64>,
    pub gpu_power_w: Option<f64>,
}

// ── Audit events ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuditFilters {
    pub limit: u32,
    pub offset: u32,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventRow {
    pub event_time: DateTime<Utc>,
    pub account_id: String,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: String,
    pub details: String,
}

// ── Analytics summary ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStat {
    pub model_name: String,
    pub request_count: u64,
    pub success_count: u64,
    pub success_rate: f64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishReasonStat {
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub avg_tps: f64,
    pub avg_prompt_tokens: f64,
    pub avg_completion_tokens: f64,
    pub models: Vec<ModelStat>,
    pub finish_reasons: Vec<FinishReasonStat>,
}

// ── Job usage ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageJob {
    pub event_time: String,
    pub request_id: String,
    pub model_name: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub finish_reason: String,
    pub status: String,
}

// ── MCP stats ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStat {
    /// Slug identifying the MCP server (matches `mcp_servers.slug`).
    pub server_slug: String,
    pub total_calls: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub cache_hit_count: u64,
    pub timeout_count: u64,
    /// Weighted average latency across all hourly buckets in the window.
    pub avg_latency_ms: f64,
}

// ── Port ───────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait AnalyticsRepository: Send + Sync {
    async fn aggregate_usage(&self, hours: u32) -> Result<UsageAggregate>;
    async fn key_usage_hourly(&self, key_id: &Uuid, hours: u32) -> Result<Vec<HourlyUsage>>;
    async fn performance(&self, hours: u32) -> Result<PerformanceMetrics>;
    async fn server_metrics_history(&self, server_id: &Uuid, hours: u32) -> Result<Vec<MetricsPoint>>;
    async fn audit_events(&self, filters: AuditFilters) -> Result<Vec<AuditEventRow>>;
    async fn analytics_summary(&self, hours: u32) -> Result<AnalyticsSummary>;
    async fn key_usage_jobs(&self, key_id: &Uuid, hours: u32) -> Result<Vec<UsageJob>>;
    async fn mcp_server_stats(&self, hours: u32) -> Result<Vec<McpServerStat>>;
}
