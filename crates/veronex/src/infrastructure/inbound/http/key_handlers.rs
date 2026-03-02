use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;

async fn emit_audit(
    state: &super::state::AppState,
    actor: &Claims,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id: actor.sub,
            account_name: actor.sub.to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            resource_name: resource_name.to_string(),
            ip_address: None,
            details: Some(details.to_string()),
        })
        .await;
    }
}

#[derive(Deserialize)]
pub struct PatchKeyRequest {
    pub is_active: Option<bool>,
    /// Billing tier: `"paid"` | `"free"`.
    pub tier: Option<String>,
}

use crate::domain::entities::ApiKey;
use crate::domain::services::api_key_generator::generate_api_key;

use super::state::AppState;

// ── Request / Response types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub tenant_id: String,
    pub name: String,
    #[serde(default)]
    pub rate_limit_rpm: i32,
    #[serde(default)]
    pub rate_limit_tpm: i32,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    /// Billing tier: `"free"` or `"paid"` (default).
    #[serde(default = "default_tier")]
    pub tier: String,
}

fn default_tier() -> String {
    "paid".to_string()
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub id: Uuid,
    pub key: String,
    pub key_prefix: String,
    pub tenant_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct KeySummary {
    pub id: Uuid,
    pub key_prefix: String,
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,
    pub rate_limit_tpm: i32,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
    /// Billing tier: `"free"` or `"paid"`.
    pub tier: String,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /v1/keys — Create a new API key.
///
/// Returns the plaintext key exactly once. It is never stored or retrievable again.
pub async fn create_key(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, StatusCode> {
    let (id, plaintext, key_hash, key_prefix) = generate_api_key();
    let now = Utc::now();

    let api_key = ApiKey {
        id,
        key_hash,
        key_prefix: key_prefix.clone(),
        tenant_id: req.tenant_id.clone(),
        name: req.name,
        is_active: true,
        rate_limit_rpm: req.rate_limit_rpm,
        rate_limit_tpm: req.rate_limit_tpm,
        expires_at: req.expires_at,
        created_at: now,
        deleted_at: None,
        key_type: "standard".to_string(),
        tier: req.tier,
    };

    state
        .api_key_repo
        .create(&api_key)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    emit_audit(&state, &claims, "create", "api_key", &id.to_string(), &api_key.name,
        &format!("API key '{}' created for tenant '{}' (tier: {}, rpm_limit: {}, tpm_limit: {})",
            api_key.name, api_key.tenant_id, api_key.tier,
            api_key.rate_limit_rpm, api_key.rate_limit_tpm)).await;

    Ok(Json(CreateKeyResponse {
        id,
        key: plaintext,
        key_prefix,
        tenant_id: req.tenant_id,
        created_at: now,
    }))
}

/// GET /v1/keys — List all keys for the authenticated tenant.
///
/// Returns key prefix only — never the hash or plaintext.
pub async fn list_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<KeySummary>>, StatusCode> {
    // Extract tenant from the authenticated API key in extensions
    // For now, list all keys (admin endpoint)
    let keys = state
        .api_key_repo
        .list_by_tenant("default")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let summaries: Vec<KeySummary> = keys
        .into_iter()
        .filter(|k| k.key_type != "test")
        .map(|k| KeySummary {
            id: k.id,
            key_prefix: k.key_prefix,
            tenant_id: k.tenant_id,
            name: k.name,
            is_active: k.is_active,
            rate_limit_rpm: k.rate_limit_rpm,
            rate_limit_tpm: k.rate_limit_tpm,
            expires_at: k.expires_at,
            created_at: k.created_at,
            tier: k.tier,
        })
        .collect();

    Ok(Json(summaries))
}

/// DELETE /v1/keys/{id} — Soft-delete an API key (hidden from list, blocked from auth).
pub async fn delete_key(
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    state
        .api_key_repo
        .soft_delete(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    emit_audit(&state, &claims, "delete", "api_key", &id, &id,
        &format!("API key {id} soft-deleted (access permanently revoked)")).await;

    Ok(StatusCode::NO_CONTENT)
}

/// PATCH /v1/keys/{id} — Update mutable fields: `is_active` and/or `tier`.
pub async fn toggle_key(
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<PatchKeyRequest>,
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Build audit description before consuming req fields.
    let mut changes = Vec::new();
    if let Some(active) = req.is_active { changes.push(format!("is_active={active}")); }
    if let Some(ref tier) = req.tier { changes.push(format!("tier={tier}")); }
    let details = if changes.is_empty() {
        format!("API key {id} updated (no changes)")
    } else {
        format!("API key {id} updated — {}", changes.join(", "))
    };

    if let Some(active) = req.is_active {
        state
            .api_key_repo
            .set_active(&uuid, active)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(tier) = req.tier {
        if tier != "paid" && tier != "free" {
            return Err(StatusCode::BAD_REQUEST);
        }
        state
            .api_key_repo
            .set_tier(&uuid, &tier)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    emit_audit(&state, &claims, "update", "api_key", &id, &id, &details).await;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_key_request_deserialization() {
        let json = serde_json::json!({
            "tenant_id": "tenant-1",
            "name": "my-key"
        });
        let req: CreateKeyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.tenant_id, "tenant-1");
        assert_eq!(req.name, "my-key");
        assert_eq!(req.rate_limit_rpm, 0);
        assert_eq!(req.rate_limit_tpm, 0);
        assert!(req.expires_at.is_none());
    }

    #[test]
    fn create_key_request_with_limits() {
        let json = serde_json::json!({
            "tenant_id": "tenant-1",
            "name": "my-key",
            "rate_limit_rpm": 60,
            "rate_limit_tpm": 100000
        });
        let req: CreateKeyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.rate_limit_rpm, 60);
        assert_eq!(req.rate_limit_tpm, 100_000);
    }

    #[test]
    fn create_key_response_serialization() {
        let resp = CreateKeyResponse {
            id: Uuid::now_v7(),
            key: "iq_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            key_prefix: "iq_01ARZ3NDEK".to_string(),
            tenant_id: "tenant-1".to_string(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("id").is_some());
        assert!(json.get("key").is_some());
        assert!(json.get("key_prefix").is_some());
        assert!(json.get("tenant_id").is_some());
        assert!(json.get("created_at").is_some());
    }

    #[test]
    fn key_summary_serialization() {
        let summary = KeySummary {
            id: Uuid::now_v7(),
            key_prefix: "iq_01ARZ3NDEK".to_string(),
            tenant_id: "tenant-1".to_string(),
            name: "test-key".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            created_at: Utc::now(),
            tier: "paid".to_string(),
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("id").is_some());
        assert!(json.get("key_prefix").is_some());
        // Should NOT contain key_hash
        assert!(json.get("key_hash").is_none());
    }
}
