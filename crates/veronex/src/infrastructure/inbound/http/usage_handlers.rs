use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsSummary, HourlyUsage, UsageAggregate, UsageJob,
};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSuper};

use super::error::AppError;
use super::query_helpers::validate_hours;
use super::state::AppState;
use super::usage_queries::{self, ModelBreakdown, UsageBreakdownResponse};

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_hours() -> u32 {
    24
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
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageAggregate>, AppError> {
    validate_hours(params.hours)?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(result) = repo.aggregate_usage(params.hours).await
            && result.request_count > 0 {
                return Ok(Json(result));
            }
    Ok(Json(usage_queries::pg_aggregate_usage(&state.pg_pool, params.hours).await?))
}

/// GET /v1/usage/{key_id} — Per-key hourly breakdown.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<HourlyUsage>>, AppError> {
    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(rows) = repo.key_usage_hourly(&uuid, params.hours).await
            && !rows.is_empty() {
                return Ok(Json(rows));
            }
    Ok(Json(usage_queries::pg_key_usage_hourly(&state.pg_pool, &uuid, params.hours).await?))
}

/// GET /v1/dashboard/analytics — Model distribution, finish reasons, TPS and avg tokens (super admin only).
/// ClickHouse primary, PostgreSQL fallback.
pub async fn get_analytics(
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<AnalyticsSummary>, AppError> {
    validate_hours(params.hours)?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(summary) = repo.analytics_summary(params.hours).await
            && !summary.models.is_empty() {
                return Ok(Json(summary));
            }
    Ok(Json(usage_queries::pg_analytics_summary(&state.pg_pool, params.hours).await?))
}

/// GET /v1/usage/{key_id}/jobs — Individual request list for a key.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage_jobs(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<UsageJob>>, AppError> {
    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(jobs) = repo.key_usage_jobs(&uuid, params.hours).await
            && !jobs.is_empty() {
                return Ok(Json(jobs));
            }
    Ok(Json(usage_queries::pg_key_usage_jobs(&state.pg_pool, &uuid, params.hours).await?))
}

/// GET /v1/usage/{key_id}/models — Per-key model breakdown from PostgreSQL.
/// Returns which models the key has used, with request counts and token stats.
pub async fn key_model_breakdown(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<ModelBreakdown>>, AppError> {
    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;

    Ok(Json(usage_queries::pg_key_model_breakdown(&state.pg_pool, &uuid, params.hours).await?))
}

/// GET /v1/usage/breakdown — Provider, API key, and model breakdown from PostgreSQL (super admin only).
pub async fn usage_breakdown(
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageBreakdownResponse>, AppError> {
    validate_hours(params.hours)?;
    Ok(Json(usage_queries::pg_usage_breakdown(&state.pg_pool, params.hours).await?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::analytics_repository::UsageAggregate;

    #[test]
    fn usage_query_defaults() {
        let json = serde_json::json!({});
        let query: UsageQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.hours, 24);
    }

    #[test]
    fn usage_query_custom_hours() {
        let json = serde_json::json!({"hours": 72});
        let query: UsageQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.hours, 72);
    }

    #[test]
    fn usage_aggregate_serialization() {
        let agg = UsageAggregate {
            request_count: 100,
            success_count: 90,
            cancelled_count: 5,
            error_count: 5,
            prompt_tokens: 10000,
            completion_tokens: 50000,
            total_tokens: 60000,
        };
        let json = serde_json::to_value(&agg).unwrap();
        assert_eq!(json["request_count"], 100);
        assert_eq!(json["total_tokens"], 60000);
    }
}
