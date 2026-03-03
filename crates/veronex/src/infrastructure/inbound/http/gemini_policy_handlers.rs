use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::domain::entities::GeminiRateLimitPolicy;
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;

use super::state::AppState;

async fn emit_audit(state: &AppState, actor: &Claims, action: &str, resource_id: &str, resource_name: &str, details: &str) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id: actor.sub,
            account_name: actor.sub.to_string(),
            action: action.to_string(),
            resource_type: "gemini_backend".to_string(),
            resource_id: resource_id.to_string(),
            resource_name: resource_name.to_string(),
            ip_address: None,
            details: Some(details.to_string()),
        })
        .await;
    }
}

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GeminiPolicySummary {
    pub id: String,
    /// e.g. "gemini-2.5-flash" or "*" (global default)
    pub model_name: String,
    pub rpm_limit: i32,
    pub rpd_limit: i32,
    /// When false: skip all free-tier providers; route directly to a paid provider.
    /// Also suppresses RPM/RPD counter increments (paid providers have no limits).
    pub available_on_free_tier: bool,
    pub updated_at: DateTime<Utc>,
}

impl From<GeminiRateLimitPolicy> for GeminiPolicySummary {
    fn from(p: GeminiRateLimitPolicy) -> Self {
        Self {
            id: p.id.to_string(),
            model_name: p.model_name,
            rpm_limit: p.rpm_limit,
            rpd_limit: p.rpd_limit,
            available_on_free_tier: p.available_on_free_tier,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpsertGeminiPolicyRequest {
    pub rpm_limit: i32,
    pub rpd_limit: i32,
    /// Defaults to true if not provided.
    #[serde(default = "default_true")]
    pub available_on_free_tier: bool,
}

fn default_true() -> bool {
    true
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /v1/gemini/policies` — list all Gemini rate-limit policies.
///
/// Returns one row per model name. The `"*"` row is the global fallback.
pub async fn list_gemini_policies(State(state): State<AppState>) -> impl IntoResponse {
    match state.gemini_policy_repo.list_all().await {
        Ok(policies) => {
            let summaries: Vec<GeminiPolicySummary> =
                policies.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(summaries)).into_response()
        }
        Err(e) => {
            tracing::error!("failed to list gemini policies: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `PUT /v1/gemini/policies/{model_name}` — create or update a per-model rate-limit policy.
///
/// Use `model_name = *` to set the global default applied to all models without
/// a model-specific row.
///
/// Example:
/// ```text
/// PUT /v1/gemini/policies/gemini-2.5-flash
/// { "rpm_limit": 10, "rpd_limit": 250 }
/// ```
pub async fn upsert_gemini_policy(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Json(req): Json<UpsertGeminiPolicyRequest>,
) -> impl IntoResponse {
    let policy = GeminiRateLimitPolicy {
        id: Uuid::now_v7(),
        model_name: model_name.clone(),
        rpm_limit: req.rpm_limit,
        rpd_limit: req.rpd_limit,
        available_on_free_tier: req.available_on_free_tier,
        updated_at: Utc::now(),
    };

    match state.gemini_policy_repo.upsert(&policy).await {
        Ok(()) => {
            tracing::info!(
                model_name = %model_name,
                rpm = req.rpm_limit,
                rpd = req.rpd_limit,
                "gemini policy upserted"
            );
            emit_audit(&state, &claims, "update", &model_name, &format!("gemini_policy:{model_name}"),
                &format!("Gemini rate-limit policy for '{}' upserted: rpm={}, rpd={}, free_tier={}",
                    model_name, req.rpm_limit, req.rpd_limit, req.available_on_free_tier)).await;
            (StatusCode::OK, Json(GeminiPolicySummary::from(policy))).into_response()
        }
        Err(e) => {
            tracing::error!("failed to upsert gemini policy: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}
