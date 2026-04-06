use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;

use super::audit_helpers::emit_audit;
use super::error::AppError;

use crate::domain::entities::ApiKey;
use crate::domain::enums::{KeyTier, KeyType};
use crate::domain::services::api_key_generator::generate_api_key;
use crate::domain::value_objects::ApiKeyId;

use super::state::AppState;

#[derive(Deserialize)]
pub struct PatchKeyRequest {
    pub is_active: Option<bool>,
    /// Billing tier: `"paid"` | `"free"`.
    pub tier: Option<String>,
    pub mcp_cap_points: Option<i16>,
}

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
    /// Billing tier: free or paid (default).
    #[serde(default)]
    pub tier: KeyTier,
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub id: ApiKeyId,
    pub key: String,
    pub key_prefix: String,
    pub tenant_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct KeySummary {
    pub id: ApiKeyId,
    pub key_prefix: String,
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,
    pub rate_limit_tpm: i32,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
    /// Billing tier: free or paid.
    pub tier: KeyTier,
    pub mcp_cap_points: i16,
    /// Username of creator (populated via account_id JOIN).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /v1/keys — Create a new API key.
///
/// Returns the plaintext key exactly once. It is never stored or retrievable again.
pub async fn create_key(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.rate_limit_rpm < 0 {
        return Err(AppError::BadRequest("rate_limit_rpm must be non-negative".into()));
    }
    if req.rate_limit_tpm < 0 {
        return Err(AppError::BadRequest("rate_limit_tpm must be non-negative".into()));
    }

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
        key_type: KeyType::Standard,
        tier: req.tier,
        mcp_cap_points: 3,
        account_id: Some(claims.sub),
    };

    state
        .api_key_repo
        .create(&api_key)
        .await?;

    emit_audit(&state, &claims, "create", "api_key", &id.to_string(), &api_key.name,
        &format!("API key '{}' created for tenant '{}' (tier: {}, rpm_limit: {}, tpm_limit: {})",
            api_key.name, api_key.tenant_id, api_key.tier,
            api_key.rate_limit_rpm, api_key.rate_limit_tpm)).await;

    Ok((StatusCode::CREATED, Json(CreateKeyResponse {
        id: ApiKeyId::from_uuid(id),
        key: plaintext,
        key_prefix,
        tenant_id: req.tenant_id,
        created_at: now,
    })))
}

/// Resolve the tenant_id (username) for the authenticated user.
pub(super) async fn resolve_tenant_id(state: &AppState, claims: &Claims) -> Result<String, AppError> {
    let account = state
        .account_repo
        .get_by_id(&claims.sub)
        .await?
        .ok_or_else(|| AppError::Forbidden("account not found".into()))?;
    Ok(account.username)
}

use super::handlers::ListPageParams;

/// GET /v1/keys — List keys for the authenticated tenant with optional search/pagination.
///
/// Returns key prefix only — never the hash or plaintext.
pub async fn list_keys(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(50).clamp(1, 1000);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;

    let (keys, total) = if claims.role == crate::domain::enums::AccountRole::Super {
        state.api_key_repo.list_page(&search, limit, offset).await?
    } else {
        let tenant_id = resolve_tenant_id(&state, &claims).await?;
        state.api_key_repo.list_by_tenant_page(&tenant_id, &search, limit, offset).await?
    };

    // Batch-resolve account_id → username in O(1) query (accounts table is small — admin only)
    let username_map: std::collections::HashMap<Uuid, String> = state
        .account_repo
        .list_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.id, a.username))
        .collect();

    let summaries: Vec<KeySummary> = keys
        .into_iter()
        .filter(|k| !k.key_type.is_test())
        .map(|k| {
            let created_by = k.account_id.and_then(|id| username_map.get(&id).cloned());
            KeySummary {
                id: ApiKeyId::from_uuid(k.id),
                key_prefix: k.key_prefix,
                tenant_id: k.tenant_id,
                name: k.name,
                is_active: k.is_active,
                rate_limit_rpm: k.rate_limit_rpm,
                rate_limit_tpm: k.rate_limit_tpm,
                expires_at: k.expires_at,
                created_at: k.created_at,
                tier: k.tier,
                mcp_cap_points: k.mcp_cap_points,
                created_by,
            }
        })
        .collect();

    Ok(Json(serde_json::json!({
        "keys": summaries,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

/// DELETE /v1/keys/{id} — Soft-delete an API key (hidden from list, blocked from auth).
pub async fn delete_key(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let key = state.api_key_repo.get_by_id(&kid.0).await?
        .ok_or_else(|| AppError::NotFound("key not found".into()))?;
    if claims.role != crate::domain::enums::AccountRole::Super {
        let tenant_id = resolve_tenant_id(&state, &claims).await?;
        if key.tenant_id != tenant_id {
            return Err(AppError::Forbidden("not your key".into()));
        }
    }

    state.api_key_repo.soft_delete(&kid.0).await?;

    emit_audit(&state, &claims, "delete", "api_key", &kid.to_string(), &kid.to_string(),
        &format!("API key {kid} soft-deleted (access permanently revoked)")).await;

    Ok(StatusCode::NO_CONTENT)
}

/// PATCH /v1/keys/{id} — Update mutable fields: `is_active` and/or `tier`.
pub async fn toggle_key(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
    Json(req): Json<PatchKeyRequest>,
) -> Result<StatusCode, AppError> {
    let key = state.api_key_repo.get_by_id(&kid.0).await?
        .ok_or_else(|| AppError::NotFound("key not found".into()))?;
    if claims.role != crate::domain::enums::AccountRole::Super {
        let tenant_id = resolve_tenant_id(&state, &claims).await?;
        if key.tenant_id != tenant_id {
            return Err(AppError::Forbidden("not your key".into()));
        }
    }

    // Validate all input before any writes to avoid partial updates.
    let tier = match req.tier {
        Some(ref s) => Some(s.parse::<KeyTier>().map_err(AppError::BadRequest)?),
        None => None,
    };
    if let Some(cap) = req.mcp_cap_points {
        if !(0..=10).contains(&cap) {
            return Err(AppError::BadRequest("mcp_cap_points must be between 0 and 10".into()));
        }
    }

    // Build audit description before consuming req fields.
    let mut changes = Vec::new();
    if let Some(active) = req.is_active { changes.push(format!("is_active={active}")); }
    if let Some(ref t) = tier { changes.push(format!("tier={t}")); }
    if let Some(cap) = req.mcp_cap_points { changes.push(format!("mcp_cap_points={cap}")); }
    let details = if changes.is_empty() {
        format!("API key {kid} updated (no changes)")
    } else {
        format!("API key {kid} updated — {}", changes.join(", "))
    };

    state.api_key_repo.update_fields(&kid.0, req.is_active, tier.as_ref()).await?;

    if let Some(cap) = req.mcp_cap_points {
        sqlx::query("UPDATE api_keys SET mcp_cap_points = $1 WHERE id = $2")
            .bind(cap)
            .bind(kid.0)
            .execute(&state.pg_pool)
            .await
            .map_err(super::error::db_error)?;
        // Invalidate Valkey cap_points cache for this key.
        if let Some(ref pool) = state.valkey_pool {
            use fred::prelude::*;
            use crate::infrastructure::outbound::valkey_keys;
            let _ = pool.del::<(), _>(&valkey_keys::mcp_key_cap_points(kid.0)).await;
        }
    }

    emit_audit(&state, &claims, "update", "api_key", &kid.to_string(), &kid.to_string(), &details).await;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/keys/{id}/regenerate — Issue a new key for the same ID.
///
/// The old key is immediately invalidated. Returns the new plaintext key once.
pub async fn regenerate_key(
    Extension(claims): Extension<Claims>,
    Path(kid): Path<ApiKeyId>,
    State(state): State<AppState>,
) -> Result<Json<CreateKeyResponse>, AppError> {
    let key = state.api_key_repo.get_by_id(&kid.0).await?
        .ok_or_else(|| AppError::NotFound("key not found".into()))?;
    if claims.role != crate::domain::enums::AccountRole::Super {
        let tenant_id = resolve_tenant_id(&state, &claims).await?;
        if key.tenant_id != tenant_id {
            return Err(AppError::Forbidden("not your key".into()));
        }
    }

    let (_new_id, plaintext, new_hash, new_prefix) = generate_api_key();
    state.api_key_repo.regenerate(&kid.0, &new_hash, &new_prefix).await?;

    emit_audit(&state, &claims, "regenerate", "api_key", &kid.to_string(), &key.name,
        &format!("API key '{}' regenerated (old key invalidated)", key.name)).await;

    Ok(Json(CreateKeyResponse {
        id: kid,
        key: plaintext,
        key_prefix: new_prefix,
        tenant_id: key.tenant_id,
        created_at: key.created_at,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
            id: ApiKeyId::new(),
            key: "vnx_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            key_prefix: "vnx_01ARZ3NDE".to_string(),
            tenant_id: "tenant-1".to_string(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        let id_str = json["id"].as_str().unwrap();
        assert!(id_str.starts_with("key_"), "expected key_ prefix, got: {id_str}");
        assert!(json.get("key").is_some());
        assert!(json.get("key_prefix").is_some());
        assert!(json.get("tenant_id").is_some());
        assert!(json.get("created_at").is_some());
    }

    #[test]
    fn key_summary_serialization() {
        let summary = KeySummary {
            id: ApiKeyId::new(),
            key_prefix: "vnx_01ARZ3NDE".to_string(),
            tenant_id: "tenant-1".to_string(),
            name: "test-key".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            created_at: Utc::now(),
            tier: KeyTier::Paid,
            mcp_cap_points: 3,
            created_by: None,
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("id").is_some());
        assert!(json.get("key_prefix").is_some());
        // Should NOT contain key_hash
        assert!(json.get("key_hash").is_none());
    }
}
