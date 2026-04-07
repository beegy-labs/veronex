//! Shared SQL helpers used by both dashboard and usage query modules.

use serde::Serialize;
use crate::domain::value_objects::JobId;

/// LATERAL JOIN for per-model pricing lookup.
/// Used by usage breakdown, key model breakdown, and dashboard job queries.
pub(super) const PRICING_LATERAL: &str = "\
LEFT JOIN LATERAL (
    SELECT input_per_1m, output_per_1m FROM model_pricing
    WHERE provider = j.provider_type
      AND (model_name = j.model_name OR model_name = '*')
    ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
    LIMIT 1
) pricing ON true";

/// Percentage with one decimal place: `(numerator / denominator * 100)` rounded to 0.1.
pub(super) fn pct(numerator: i64, denominator: i64) -> f64 {
    if denominator > 0 {
        (numerator as f64 / denominator as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    }
}

/// Validate hours parameter to prevent SQL INTERVAL abuse.
pub(super) fn validate_hours(hours: u32) -> Result<(), super::error::AppError> {
    if hours == 0 || hours > 8760 {
        return Err(super::error::AppError::BadRequest("hours must be between 1 and 8760".into()));
    }
    Ok(())
}

/// Compute tokens-per-second for a job.
pub(super) fn compute_tps(
    latency_ms: Option<i32>,
    ttft_ms: Option<i32>,
    completion_tokens: Option<i32>,
) -> Option<f64> {
    let tokens = completion_tokens? as f64;
    let lat = latency_ms? as f64;
    let gen_ms = lat - ttft_ms.unwrap_or(0) as f64;
    if gen_ms > 0.0 && tokens > 0.0 {
        Some((tokens * 1000.0 / gen_ms * 10.0).round() / 10.0)
    } else {
        None
    }
}

/// Common fields extracted from an `inference_jobs` row.
/// Both `JobSummary` (list) and `JobDetail` (single) share these columns.
pub(super) struct JobRowCommon {
    pub id: uuid::Uuid,
    pub model_name: String,
    pub provider_type: String,
    pub status: String,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub latency_ms: Option<i32>,
    pub ttft_ms: Option<i32>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub cached_tokens: Option<i32>,
    pub api_key_name: Option<String>,
    pub account_name: Option<String>,
    pub request_path: Option<String>,
    pub estimated_cost_usd: Option<f64>,
    pub conversation_id: Option<uuid::Uuid>,
}

impl JobRowCommon {
    // Note: unwrap_or_default used intentionally for dashboard resilience.
    // Individual row corruption should not break the dashboard list view.
    // Schema mismatches will surface as empty/default values in the UI.
    pub fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        use sqlx::Row;
        Self {
            id:                row.try_get("id").unwrap_or_default(),
            model_name:        row.try_get("model_name").unwrap_or_default(),
            provider_type:     row.try_get("provider_type").unwrap_or_default(),
            status:            row.try_get("status").unwrap_or_default(),
            source:            row.try_get("source").unwrap_or_else(|_| "api".to_string()),
            created_at:        row.try_get("created_at").unwrap_or_default(),
            completed_at:      row.try_get("completed_at").unwrap_or(None),
            latency_ms:        row.try_get("latency_ms").unwrap_or(None),
            ttft_ms:           row.try_get("ttft_ms").unwrap_or(None),
            prompt_tokens:     row.try_get("prompt_tokens").unwrap_or(None),
            completion_tokens: row.try_get("completion_tokens").unwrap_or(None),
            cached_tokens:     row.try_get("cached_tokens").unwrap_or(None),
            api_key_name:      row.try_get("api_key_name").unwrap_or(None),
            account_name:      row.try_get("account_name").unwrap_or(None),
            request_path:      row.try_get("request_path").unwrap_or(None),
            estimated_cost_usd: row.try_get("estimated_cost_usd").unwrap_or(None),
            conversation_id:   row.try_get("conversation_id").unwrap_or(None),
        }
    }

    pub fn tps(&self) -> Option<f64> {
        compute_tps(self.latency_ms, self.ttft_ms, self.completion_tokens)
    }
}

/// DTO for job summary rows returned from SQL queries.
#[derive(Serialize)]
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub provider_type: String,
    pub status: String,
    pub source: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    /// Tokens per second (generation only, excluding TTFT).
    pub tps: Option<f64>,
    pub api_key_name: Option<String>,
    /// For test run jobs: the account that submitted the job.
    pub account_name: Option<String>,
    /// HTTP path of the inbound request, e.g. "/v1/chat/completions".
    pub request_path: Option<String>,
    /// True when the model responded with tool calls instead of (or in addition to) text.
    pub has_tool_calls: bool,
    /// Estimated API cost in USD. $0.00 for Ollama (self-hosted). None = no pricing data.
    pub estimated_cost_usd: Option<f64>,
    /// Name of the provider (Ollama server) that processed this job.
    pub provider_name: Option<String>,
    /// Conversation this job belongs to (multi-turn), if any.
    pub conversation_id: Option<String>,
}

/// Build a `JobSummary` from a `JobRowCommon` and a `has_tool_calls` flag.
pub(super) fn job_summary_from_common(c: JobRowCommon, has_tool_calls: bool, provider_name: Option<String>) -> JobSummary {
    let tps = c.tps();
    JobSummary {
        id: JobId::from_uuid(c.id).to_string(),
        model_name: c.model_name,
        provider_type: c.provider_type,
        status: c.status,
        source: c.source,
        created_at: c.created_at.to_rfc3339(),
        completed_at: c.completed_at.map(|dt| dt.to_rfc3339()),
        latency_ms: c.latency_ms.map(|v| v as i64),
        ttft_ms: c.ttft_ms.map(|v| v as i64),
        prompt_tokens: c.prompt_tokens.map(|v| v as i64),
        completion_tokens: c.completion_tokens.map(|v| v as i64),
        cached_tokens: c.cached_tokens.map(|v| v as i64),
        tps,
        api_key_name: c.api_key_name,
        account_name: c.account_name,
        request_path: c.request_path,
        has_tool_calls,
        estimated_cost_usd: c.estimated_cost_usd,
        provider_name,
        conversation_id: c.conversation_id.map(|id| id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_normal_ratio() {
        assert_eq!(pct(1, 4), 25.0);
        assert_eq!(pct(1, 3), 33.3);
        assert_eq!(pct(2, 3), 66.7);
    }

    #[test]
    fn pct_zero_denominator_returns_zero() {
        assert_eq!(pct(100, 0), 0.0);
    }

    #[test]
    fn pct_full_coverage() {
        assert_eq!(pct(10, 10), 100.0);
    }

    #[test]
    fn validate_hours_rejects_zero() {
        assert!(validate_hours(0).is_err());
    }

    #[test]
    fn validate_hours_rejects_over_8760() {
        assert!(validate_hours(8761).is_err());
    }

    #[test]
    fn validate_hours_accepts_boundaries() {
        assert!(validate_hours(1).is_ok());
        assert!(validate_hours(8760).is_ok());
    }

    #[test]
    fn compute_tps_normal() {
        // 100 tokens, latency 2000ms, ttft 500ms → gen_ms=1500 → tps=66.7
        let tps = compute_tps(Some(2000), Some(500), Some(100)).unwrap();
        assert_eq!(tps, 66.7);
    }

    #[test]
    fn compute_tps_none_when_missing_fields() {
        assert!(compute_tps(None, None, Some(100)).is_none());
        assert!(compute_tps(Some(1000), None, None).is_none());
    }

    #[test]
    fn compute_tps_none_when_gen_ms_zero() {
        // ttft == latency → gen_ms = 0 → no TPS
        assert!(compute_tps(Some(500), Some(500), Some(100)).is_none());
    }
}
