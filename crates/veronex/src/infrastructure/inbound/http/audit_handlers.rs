use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::AuditFilters;
use crate::infrastructure::inbound::http::error::AppError;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSuper;
use crate::infrastructure::inbound::http::state::AppState;

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
}

#[derive(Serialize)]
pub struct AuditEventResponse {
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

// ── GET /v1/audit ─────────────────────────────────────────────────────────────

pub async fn list_audit_events(
    RequireSuper(_claims): RequireSuper,
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEventResponse>>, AppError> {
    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or_else(|| AppError::ServiceUnavailable("analytics not configured".into()))?;

    let filters = AuditFilters {
        limit: q.limit.unwrap_or(100).min(1000),
        offset: q.offset.unwrap_or(0),
        action: q.action,
        resource_type: q.resource_type,
        resource_id: q.resource_id,
    };

    let rows = repo
        .audit_events(filters)
        .await?;

    let events = rows
        .into_iter()
        .map(|r| AuditEventResponse {
            event_time: r.event_time,
            account_id: r.account_id,
            account_name: r.account_name,
            action: r.action,
            resource_type: r.resource_type,
            resource_id: r.resource_id,
            resource_name: r.resource_name,
            ip_address: r.ip_address,
            details: r.details,
        })
        .collect();

    Ok(Json(events))
}
