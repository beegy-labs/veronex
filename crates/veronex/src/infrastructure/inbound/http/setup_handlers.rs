//! First-install wizard endpoints (extends `/v1/setup/*`).
//!
//! Two-step setup flow for an open-source self-host:
//!
//! 1. `POST /v1/setup`         — admin account (existing, in `auth_handlers`).
//! 2. `POST /v1/setup/storage` — Garage S3 + HuggingFace + Local PV settings.
//!
//! After both steps succeed the server returns `{restart_required: true}`
//! and the operator restarts; on next boot bootstrap reads the config from
//! Postgres and instantiates the Phase 2 components, after which admin
//! endpoints transition from 503 to live behaviour. Hot-reload of S3 keys
//! deliberately requires restart — connection clients live across the
//! Phase 2 module graph and we don't want partial-rebind glitches.
//!
//! Admin runtime read/write endpoints (`/v1/admin/config`) let an operator
//! update individual keys later without going through the wizard.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::application::ports::outbound::app_config_repository::{
    self as cfg, AppConfigRepository, AppConfigUpsert, ALL_KEYS,
};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::inbound::http::state::AppState;

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Combined setup status: account + storage. Frontend uses both flags to
/// pick which wizard step to show.
#[derive(Debug, Serialize)]
pub struct SetupStatusV2Response {
    /// True until the first super admin is created.
    pub needs_setup_account: bool,
    /// True until the storage wizard (S3 endpoint + bucket + access keys
    /// + secret key) records all required keys.
    pub needs_setup_storage: bool,
    /// True iff both above are false. Frontend redirects to dashboard.
    pub setup_complete: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetupStorageRequest {
    pub s3_endpoint: String,
    #[serde(default)]
    pub s3_region: Option<String>,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_model_bucket: String,
    /// Optional HuggingFace bearer token for private repos / rate limits.
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default)]
    pub hf_endpoint: Option<String>,
    #[serde(default)]
    pub model_local_path: Option<String>,
    #[serde(default)]
    pub model_max_disk_gb: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigUpsertRequest {
    pub value: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Required-for-storage keys, paired with the env var the bootstrap
/// resolver checks first. The status helper considers env > DB so a
/// docker-compose deployment that pins the values via env reports
/// `needs_setup_storage = false` even when `app_config` is empty.
const REQUIRED_STORAGE_KEYS: &[(&str, &str)] = &[
    (cfg::KEY_S3_ENDPOINT, "S3_ENDPOINT"),
    (cfg::KEY_S3_ACCESS_KEY, "S3_ACCESS_KEY"),
    (cfg::KEY_S3_SECRET_KEY, "S3_SECRET_KEY"),
    (cfg::KEY_S3_MODEL_BUCKET, "VERONEX_MODEL_BUCKET"),
];

/// True when every required storage key resolves to a non-empty value
/// from env or DB. Mirrors `bootstrap::model_store::resolve` so the
/// wizard UI agrees with the live AppState wiring.
async fn storage_configured(repo: &dyn AppConfigRepository) -> bool {
    let cfg_keys: Vec<&str> = REQUIRED_STORAGE_KEYS.iter().map(|(k, _)| *k).collect();
    let entries = match repo.get_many(&cfg_keys).await {
        Ok(e) => e,
        Err(_) => return false,
    };
    let by_key: std::collections::HashMap<_, _> =
        entries.into_iter().map(|e| (e.key.clone(), e)).collect();
    REQUIRED_STORAGE_KEYS.iter().all(|(cfg_key, env_key)| {
        let env_val = std::env::var(env_key).ok().filter(|v| !v.is_empty());
        let db_val = by_key
            .get(*cfg_key)
            .and_then(|e| e.value.as_deref())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());
        env_val.or(db_val).is_some()
    })
}

fn not_wired() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "app_config_not_wired",
            "message": "AppConfigRepository is not configured. Set VERONEX_ENCRYPTION_KEY and restart.",
        })),
    )
}

// ── Endpoints ───────────────────────────────────────────────────────────────

/// `GET /v1/setup/status` (v2 shape, replaces the v1 single-flag response).
/// No authentication: needs to work before the first account exists.
pub async fn setup_status_v2(State(state): State<AppState>) -> impl IntoResponse {
    let needs_account = state
        .account_repo
        .list_all()
        .await
        .map(|a| a.is_empty())
        .unwrap_or(true);

    let needs_storage = match state.app_config_repo.as_ref() {
        Some(repo) => !storage_configured(repo.as_ref()).await,
        // No app_config available at all — operator must run with env vars
        // set. We surface needs_storage = true so the wizard appears.
        None => true,
    };

    Json(SetupStatusV2Response {
        needs_setup_account: needs_account,
        needs_setup_storage: needs_storage,
        setup_complete: !needs_account && !needs_storage,
    })
    .into_response()
}

/// `POST /v1/setup/storage` — gather S3/HF/Local PV settings in one shot.
///
/// Auth policy: requires the same ProviderManage permission used by other
/// admin endpoints. Bootstrap allows the very first super admin to call
/// this immediately after `/v1/setup`. We do NOT allow an unauthenticated
/// call because anyone reaching the open-source instance pre-config could
/// otherwise hijack the storage layer.
pub async fn setup_storage(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<SetupStorageRequest>,
) -> impl IntoResponse {
    let Some(repo) = state.app_config_repo.as_ref() else {
        return not_wired().into_response();
    };

    // Validate the obvious — frontend should already have caught these but
    // a malformed direct API call shouldn't succeed.
    if req.s3_endpoint.trim().is_empty() {
        return bad_request("s3_endpoint is required");
    }
    if req.s3_access_key.trim().is_empty()
        || req.s3_secret_key.trim().is_empty()
        || req.s3_model_bucket.trim().is_empty()
    {
        return bad_request("s3 access key / secret key / bucket are required");
    }
    if let Some(gb) = req.model_max_disk_gb {
        if gb == 0 {
            return bad_request("model_max_disk_gb must be > 0 (or omit it for default)");
        }
    }

    let by = Some(claims.sub);

    let mut entries: Vec<AppConfigUpsert> = vec![
        AppConfigUpsert {
            key: cfg::KEY_S3_ENDPOINT.into(),
            value: req.s3_endpoint.trim().to_string(),
            is_secret: false,
        },
        AppConfigUpsert {
            key: cfg::KEY_S3_ACCESS_KEY.into(),
            value: req.s3_access_key.trim().to_string(),
            is_secret: false,
        },
        AppConfigUpsert {
            key: cfg::KEY_S3_SECRET_KEY.into(),
            value: req.s3_secret_key,
            is_secret: true,
        },
        AppConfigUpsert {
            key: cfg::KEY_S3_MODEL_BUCKET.into(),
            value: req.s3_model_bucket.trim().to_string(),
            is_secret: false,
        },
    ];
    if let Some(region) = req.s3_region {
        entries.push(AppConfigUpsert {
            key: cfg::KEY_S3_REGION.into(),
            value: region,
            is_secret: false,
        });
    }
    if let Some(token) = req.hf_token {
        if !token.trim().is_empty() {
            entries.push(AppConfigUpsert {
                key: cfg::KEY_HF_TOKEN.into(),
                value: token,
                is_secret: true,
            });
        }
    }
    if let Some(endpoint) = req.hf_endpoint {
        entries.push(AppConfigUpsert {
            key: cfg::KEY_HF_ENDPOINT.into(),
            value: endpoint,
            is_secret: false,
        });
    }
    if let Some(path) = req.model_local_path {
        entries.push(AppConfigUpsert {
            key: cfg::KEY_MODEL_LOCAL_PATH.into(),
            value: path,
            is_secret: false,
        });
    }
    if let Some(gb) = req.model_max_disk_gb {
        entries.push(AppConfigUpsert {
            key: cfg::KEY_MODEL_MAX_DISK_GB.into(),
            value: gb.to_string(),
            is_secret: false,
        });
    }

    if let Err(e) = repo.upsert_many(&entries, by).await {
        tracing::error!(error = %e, "setup_storage upsert failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "upsert_failed", "message": e.to_string() })),
        )
            .into_response();
    }

    // The new config is in DB but the live AppState still holds the old
    // Phase 2 wiring (None at first boot). Tell the operator to restart so
    // bootstrap re-reads and constructs BlobStore / LocalPv / etc.
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "restart_required": true,
            "message": "Storage configured. Restart Veronex to apply.",
        })),
    )
        .into_response()
}

// ── Admin runtime config endpoints ──────────────────────────────────────────

/// `GET /v1/admin/config` — list every key. Secret values are masked
/// (replaced with `"********"`); the timestamps and `is_secret` flag are
/// always returned so the UI can render an editor without revealing the
/// stored value.
pub async fn list_config(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(repo) = state.app_config_repo.as_ref() else {
        return not_wired().into_response();
    };
    let rows = match repo.list_all().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "list_failed", "message": e.to_string() })),
            )
                .into_response();
        }
    };
    // Index by key for easy lookup; fold in canonical keys that don't yet
    // have a row so the UI can render an empty input for them.
    let mut by_key: std::collections::HashMap<String, _> =
        rows.into_iter().map(|e| (e.key.clone(), e)).collect();
    let mut items: Vec<serde_json::Value> = Vec::new();
    for (k, sec) in ALL_KEYS {
        let row = by_key.remove(*k);
        let value = row.as_ref().and_then(|e| e.value.clone());
        let display = if *sec {
            value.as_deref().filter(|v| !v.is_empty()).map(|_| "********".to_string())
        } else {
            value
        };
        items.push(json!({
            "key": k,
            "value": display,
            "is_secret": sec,
            "set": row.is_some(),
            "updated_at": row.as_ref().map(|e| e.updated_at),
            "updated_by": row.as_ref().and_then(|e| e.updated_by),
        }));
    }
    // Append any non-canonical keys the operator added directly via SQL.
    for (_, e) in by_key {
        let display = if e.is_secret {
            e.value.as_ref().filter(|v| !v.is_empty()).map(|_| "********".to_string())
        } else {
            e.value.clone()
        };
        items.push(json!({
            "key": e.key,
            "value": display,
            "is_secret": e.is_secret,
            "set": true,
            "updated_at": e.updated_at,
            "updated_by": e.updated_by,
        }));
    }
    (StatusCode::OK, Json(json!({ "config": items }))).into_response()
}

/// `PATCH /v1/admin/config/:key` — upsert a single key.
/// Honors the canonical secret-flag: a key listed in `ALL_KEYS` keeps its
/// declared `is_secret`; arbitrary user-defined keys are always stored as
/// non-secret.
pub async fn upsert_config(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<ConfigUpsertRequest>,
) -> impl IntoResponse {
    let Some(repo) = state.app_config_repo.as_ref() else {
        return not_wired().into_response();
    };
    let entry = AppConfigUpsert {
        key: key.clone(),
        value: req.value,
        is_secret: cfg::is_secret_key(&key),
    };
    match repo.upsert(&entry, Some(claims.sub)).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true, "key": key })))
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "upsert_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `DELETE /v1/admin/config/:key`.
pub async fn delete_config(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let Some(repo) = state.app_config_repo.as_ref() else {
        return not_wired().into_response();
    };
    match repo.delete(&key).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "key_not_found", "key": key })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "delete_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

fn bad_request(message: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "bad_request", "message": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_storage_keys_are_listed_in_canonical_set() {
        for (cfg_key, _env_key) in REQUIRED_STORAGE_KEYS {
            assert!(
                ALL_KEYS.iter().any(|(canonical, _)| canonical == cfg_key),
                "REQUIRED_STORAGE_KEYS contains {cfg_key} which is not in ALL_KEYS"
            );
        }
    }

    #[test]
    fn secret_classification_matches_canonical_set() {
        assert!(cfg::is_secret_key(cfg::KEY_S3_SECRET_KEY));
        assert!(cfg::is_secret_key(cfg::KEY_HF_TOKEN));
        assert!(!cfg::is_secret_key(cfg::KEY_S3_ACCESS_KEY));
        assert!(!cfg::is_secret_key(cfg::KEY_S3_ENDPOINT));
        assert!(!cfg::is_secret_key("unknown.key"));
    }
}
