use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::DateTime;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsRepository, AnalyticsSummary, AuditEventRow, AuditFilters, HourlyUsage,
    McpServerStat, McpToolCallEvent, MetricsPoint, PerformanceMetrics, UsageAggregate, UsageJob,
};

/// HTTP client that delegates all analytics queries to the `veronex-analytics`
/// internal service.
///
/// All errors are propagated as `anyhow::Error`; the callers map them to HTTP
/// status codes.
pub struct HttpAnalyticsClient {
    http: reqwest::Client,
    base_url: String,
    secret: String,
}

impl HttpAnalyticsClient {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            http: client,
            base_url: base_url.into(),
            secret: secret.into(),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.secret)
            .send()
            .await
            .map_err(|e| anyhow!("analytics request to {url} failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "analytics service returned {} for {}",
                resp.status(),
                url
            ));
        }

        resp.json::<T>()
            .await
            .map_err(|e| anyhow!("analytics response parse failed: {e}"))
    }
}

// ── Shared response shapes (must match veronex-analytics JSON) ─────────────────

#[derive(serde::Deserialize)]
struct AuditEventJson {
    pub event_time: String,
    pub account_id: String,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: String,
    pub details: String,
}

#[async_trait]
impl AnalyticsRepository for HttpAnalyticsClient {
    async fn aggregate_usage(&self, hours: u32) -> Result<UsageAggregate> {
        self.get(&format!("/internal/usage?hours={hours}")).await
    }

    async fn key_usage_hourly(&self, key_id: &Uuid, hours: u32) -> Result<Vec<HourlyUsage>> {
        self.get(&format!("/internal/usage/{key_id}?hours={hours}")).await
    }

    async fn performance(&self, hours: u32) -> Result<PerformanceMetrics> {
        self.get(&format!("/internal/performance?hours={hours}")).await
    }

    async fn server_metrics_history(
        &self,
        server_id: &Uuid,
        hours: u32,
    ) -> Result<Vec<MetricsPoint>> {
        self.get(&format!("/internal/metrics/history/{server_id}?hours={hours}"))
            .await
    }

    async fn audit_events(&self, filters: AuditFilters) -> Result<Vec<AuditEventRow>> {
        let mut qs = format!(
            "/internal/audit?limit={}&offset={}",
            filters.limit, filters.offset
        );
        if let Some(ref action) = filters.action {
            qs.push_str(&format!("&action={}", urlencoding(action)));
        }
        if let Some(ref rt) = filters.resource_type {
            qs.push_str(&format!("&resource_type={}", urlencoding(rt)));
        }
        if let Some(ref rid) = filters.resource_id {
            qs.push_str(&format!("&resource_id={}", urlencoding(rid)));
        }

        let raw: Vec<AuditEventJson> = self.get(&qs).await?;

        let rows = raw
            .into_iter()
            .map(|r| {
                let event_time = DateTime::parse_from_rfc3339(&r.event_time)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());
                AuditEventRow {
                    event_time,
                    account_id: r.account_id,
                    account_name: r.account_name,
                    action: r.action,
                    resource_type: r.resource_type,
                    resource_id: r.resource_id,
                    resource_name: r.resource_name,
                    ip_address: r.ip_address,
                    details: r.details,
                }
            })
            .collect();

        Ok(rows)
    }

    async fn analytics_summary(&self, hours: u32) -> Result<AnalyticsSummary> {
        self.get(&format!("/internal/analytics?hours={hours}")).await
    }

    async fn key_usage_jobs(&self, key_id: &Uuid, hours: u32) -> Result<Vec<UsageJob>> {
        self.get(&format!("/internal/usage/{key_id}/jobs?hours={hours}"))
            .await
    }

    async fn mcp_server_stats(&self, hours: u32) -> Result<Vec<McpServerStat>> {
        self.get(&format!("/internal/mcp/stats?hours={hours}")).await
    }

    async fn ingest_mcp_tool_call(&self, event: McpToolCallEvent) {
        let url = format!("{}/internal/ingest/mcp", self.base_url);
        let body = serde_json::json!({
            "event_time":      event.event_time,
            "request_id":      event.request_id,
            "api_key_id":      event.api_key_id,
            "tenant_id":       event.tenant_id,
            "server_id":       event.server_id,
            "server_slug":     event.server_slug,
            "tool_name":       event.tool_name,
            "namespaced_name": event.namespaced_name,
            "outcome":         event.outcome,
            "cache_hit":       event.cache_hit,
            "latency_ms":      event.latency_ms,
            "result_bytes":    event.result_bytes,
            "cap_charged":     event.cap_charged,
            "loop_round":      event.loop_round,
        });
        if let Err(e) = self.http.post(&url)
            .bearer_auth(&self.secret)
            .json(&body)
            .send()
            .await
        {
            tracing::warn!(error = %e, "mcp tool call ingest failed");
        }
    }
}

// Very simple percent-encoding for query param values.
fn urlencoding(s: &str) -> String {
    s.replace('%', "%25")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace(' ', "%20")
}
