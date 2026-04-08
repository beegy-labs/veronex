use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsSummary, HourlyUsage, UsageAggregate, UsageJob,
};
use crate::domain::value_objects::ApiKeyId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireDashboardView};

use super::error::AppError;
use super::query_helpers::validate_hours;
use super::state::AppState;
use super::usage_queries::{self, ModelBreakdown, UsageBreakdownResponse};

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsageQuery {
    /// Legacy: fixed hours window (1–8760). Ignored when `from` is set.
    #[serde(default = "default_hours")]
    pub hours: u32,
    /// ISO-8601 start time (e.g. "2026-03-20T00:00:00Z"). Overrides `hours`.
    pub from: Option<String>,
    /// ISO-8601 end time. Defaults to now when `from` is set but `to` is absent.
    pub to: Option<String>,
}

fn default_hours() -> u32 {
    24
}

impl UsageQuery {
    /// Resolve effective hours: if `from` is present, compute delta; else use `hours` field.
    pub fn effective_hours(&self) -> Result<u32, super::error::AppError> {
        if let Some(ref from_str) = self.from {
            let from = chrono::DateTime::parse_from_rfc3339(from_str)
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(from_str, "%Y-%m-%dT%H:%M:%S")
                    .map(|dt| dt.and_utc().fixed_offset()))
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(from_str, "%Y-%m-%dT%H:%M")
                    .map(|dt| dt.and_utc().fixed_offset()))
                .map_err(|_| super::error::AppError::BadRequest(
                    "invalid 'from' format — use ISO-8601 (e.g. 2026-03-20T00:00:00Z)".into(),
                ))?;
            let to = if let Some(ref to_str) = self.to {
                chrono::DateTime::parse_from_rfc3339(to_str)
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(to_str, "%Y-%m-%dT%H:%M:%S")
                        .map(|dt| dt.and_utc().fixed_offset()))
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(to_str, "%Y-%m-%dT%H:%M")
                        .map(|dt| dt.and_utc().fixed_offset()))
                    .map_err(|_| super::error::AppError::BadRequest(
                        "invalid 'to' format — use ISO-8601".into(),
                    ))?
            } else {
                chrono::Utc::now().fixed_offset()
            };
            let diff = to.signed_duration_since(from);
            let hours = (diff.num_hours().max(1) as u32).clamp(1, 8760);
            Ok(hours)
        } else {
            Ok(self.hours)
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Verify the key belongs to the authenticated user.
async fn verify_key_ownership(
    state: &AppState,
    claims: &Claims,
    key_id: &uuid::Uuid,
) -> Result<(), AppError> {
    // Super admin can access any key's usage
    if claims.role == crate::domain::enums::AccountRole::Super {
        // Just verify the key exists
        state.api_key_repo.get_by_id(key_id).await?
            .ok_or_else(|| AppError::NotFound("key not found".into()))?;
        return Ok(());
    }
    let tenant_id = super::key_handlers::resolve_tenant_id(state, claims).await?;
    let key = state
        .api_key_repo
        .get_by_id(key_id)
        .await?
        .ok_or_else(|| AppError::NotFound("key not found".into()))?;
    if key.tenant_id != tenant_id {
        return Err(AppError::Forbidden("access denied".into()));
    }
    Ok(())
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/usage — Aggregate usage across all keys (super admin only).
/// ClickHouse primary, PostgreSQL fallback.
pub async fn aggregate_usage(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageAggregate>, AppError> {
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(result) = repo.aggregate_usage(hours).await
            && result.request_count > 0 {
                return Ok(Json(result));
            }
    Ok(Json(usage_queries::pg_aggregate_usage(&state.pg_pool, hours).await?))
}

/// GET /v1/usage/{key_id} — Per-key hourly breakdown.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<HourlyUsage>>, AppError> {
    let key_id = kid.0;
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    verify_key_ownership(&state, &claims, &key_id).await?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(rows) = repo.key_usage_hourly(&key_id, hours).await
            && !rows.is_empty() {
                return Ok(Json(rows));
            }
    Ok(Json(usage_queries::pg_key_usage_hourly(&state.pg_pool, &key_id, hours).await?))
}

/// GET /v1/dashboard/analytics — Model distribution, finish reasons, TPS and avg tokens (super admin only).
/// ClickHouse primary, PostgreSQL fallback.
pub async fn get_analytics(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<AnalyticsSummary>, AppError> {
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(summary) = repo.analytics_summary(hours).await
            && !summary.models.is_empty() {
                return Ok(Json(summary));
            }
    Ok(Json(usage_queries::pg_analytics_summary(&state.pg_pool, hours).await?))
}

/// GET /v1/usage/{key_id}/jobs — Individual request list for a key.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage_jobs(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<UsageJob>>, AppError> {
    let key_id = kid.0;
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    verify_key_ownership(&state, &claims, &key_id).await?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(jobs) = repo.key_usage_jobs(&key_id, hours).await
            && !jobs.is_empty() {
                return Ok(Json(jobs));
            }
    Ok(Json(usage_queries::pg_key_usage_jobs(&state.pg_pool, &key_id, hours).await?))
}

/// GET /v1/usage/{key_id}/models — Per-key model breakdown from PostgreSQL.
/// Returns which models the key has used, with request counts and token stats.
pub async fn key_model_breakdown(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<ModelBreakdown>>, AppError> {
    let key_id = kid.0;
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    verify_key_ownership(&state, &claims, &key_id).await?;

    Ok(Json(usage_queries::pg_key_model_breakdown(&state.pg_pool, &key_id, hours).await?))
}

/// GET /v1/usage/breakdown — Provider, API key, and model breakdown from PostgreSQL (super admin only).
pub async fn usage_breakdown(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageBreakdownResponse>, AppError> {
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    Ok(Json(usage_queries::pg_usage_breakdown(&state.pg_pool, hours).await?))
}

