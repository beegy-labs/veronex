//! HTTP admin surface for the Modelfile registry + CAS blob store (Phase 2).
//!
//! Endpoints (mounted by `router.rs`):
//!
//! ```text
//! POST   /v1/admin/models                      register
//! GET    /v1/admin/models                      list (filter by family / status)
//! GET    /v1/admin/models/:id                  detail
//! DELETE /v1/admin/models/:id                  delete + decrement blob ref_count
//! POST   /v1/admin/models/:id/promote          family default
//! GET    /v1/admin/models/:id/install/attempts list attempt history
//! POST   /v1/admin/models/:id/install/retry    new attempt (failed only)
//! POST   /v1/admin/models/:id/install/cancel   cancel in-flight attempt
//! GET    /v1/admin/models/:id/install/stream   SSE event stream
//!
//! GET    /v1/admin/blobs                       list orphans
//! GET    /v1/admin/blobs/:sha256               detail
//! DELETE /v1/admin/blobs/:sha256               delete (404 if ref_count > 0)
//! ```
//!
//! Phase 2 omits PATCH and multipart upload (`POST .../upload`) — those join
//! when the AppState DI is fully wired. When the registry/orchestrator is
//! `None` (DI not yet hooked), every endpoint returns 503.
//!
//! `Modelfile spec` and `install_status` semantics are owned by Phase 2's
//! domain layer (`domain::entities::modelfile`). Handlers translate JSON to
//! domain types; they do not enforce any business rules of their own.

use std::convert::Infallible;
use std::time::Duration;
use tracing::instrument;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use futures::stream::StreamExt as _;
use serde::Deserialize;
use serde_json::json;

use crate::application::ports::outbound::blob_registry::OrphanFilter;
use crate::application::ports::outbound::install_attempts_log::AttemptsFilter;
use crate::application::ports::outbound::modelfile_registry::{ListFilter, ModelfilePatch};
use crate::domain::entities::{InstallStatus, TriggeredBy, VeronexModel};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::model_store::install_orchestrator::InstallEvent;

/// Shared 503 payload used whenever Phase 2 wiring isn't available.
fn not_wired() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "modelfile_registry_not_configured",
            "message": "Modelfile registry / blob store / orchestrator wiring is not enabled in this build.",
        })),
    )
}

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Request body for `POST /v1/admin/models`.
///
/// Mirrors the Modelfile shape from the SDD. `source` is a free-form JSON
/// blob (`{"type":"hf",...}`) — its actual decoding is the orchestrator's
/// job; the handler just stores it on the row.
#[derive(Debug, Deserialize)]
pub struct RegisterModelRequest {
    pub family: String,
    pub quantization: String,
    pub source: serde_json::Value,
    pub runtime: serde_json::Value,
    pub defaults: serde_json::Value,
    pub chat_template: serde_json::Value,
    #[serde(default)]
    pub stop_tokens: Option<serde_json::Value>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// sha256 the operator believes the GGUF will hash to. Used for the
    /// initial row insert; the orchestrator will overwrite if the actual
    /// stream produces a different value (logged as Sha256Mismatch).
    pub blob_sha256: String,
}

#[derive(Debug, Deserialize)]
pub struct ListModelsQuery {
    pub family: Option<String>,
    pub install_status: Option<String>,
    pub is_default: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListAttemptsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct OrphanQuery {
    pub min_orphan_days: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn model_to_json(m: &VeronexModel) -> serde_json::Value {
    json!({
        "model_id": m.model_id,
        "family": m.family,
        "quantization": m.quantization,
        "blob_sha256": m.blob_sha256,
        "display_name": m.display_name,
        "source_spec": m.source_spec,
        "runtime": m.runtime,
        "defaults": m.defaults,
        "chat_template": m.chat_template,
        "stop_tokens": m.stop_tokens,
        "system_prompt": m.system_prompt,
        "is_default": m.is_default,
        "install_status": m.install_status.as_str(),
        "last_error_kind": m.last_error_kind,
        "last_error_message": m.last_error_message,
        "last_attempt_at": m.last_attempt_at,
        "tags": m.tags,
        "created_at": m.created_at,
        "updated_at": m.updated_at,
    })
}

fn parse_install_status(s: &str) -> Option<InstallStatus> {
    s.parse().ok()
}

// ── Models endpoints ────────────────────────────────────────────────────────

/// `POST /v1/admin/models` — create row. Phase 2 leaves the actual install
/// pipeline to the orchestrator; clients that want the full happy path
/// invoke `POST .../install/retry` after registration completes if desired.
#[instrument(skip_all)]
pub async fn register_model(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<RegisterModelRequest>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };

    let model_id = format!("{}:{}", req.family, req.quantization);
    let now = Utc::now();
    let model = VeronexModel {
        model_id: model_id.clone(),
        family: req.family,
        quantization: req.quantization,
        blob_sha256: req.blob_sha256,
        display_name: req.display_name,
        source_spec: req.source,
        runtime: req.runtime,
        defaults: req.defaults,
        chat_template: req.chat_template,
        stop_tokens: req.stop_tokens,
        system_prompt: req.system_prompt,
        is_default: req.is_default,
        install_status: InstallStatus::Pending,
        last_error_kind: None,
        last_error_message: None,
        last_attempt_at: None,
        tags: req.tags,
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = registry.create(&model).await {
        return internal_err("create_failed", e);
    }
    (StatusCode::CREATED, Json(json!({ "model_id": model_id }))).into_response()
}

#[instrument(skip_all)]

pub async fn list_models(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Query(q): Query<ListModelsQuery>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };

    let filter = ListFilter {
        family: q.family,
        install_status: q.install_status.as_deref().and_then(parse_install_status),
        is_default: q.is_default,
        limit: q.limit,
        offset: q.offset,
    };
    match registry.list(&filter).await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(model_to_json).collect();
            (StatusCode::OK, Json(json!({ "models": items }))).into_response()
        }
        Err(e) => internal_err("list_failed", e),
    }
}

#[instrument(skip_all)]

pub async fn get_model(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };
    match registry.get(&id).await {
        Ok(Some(m)) => (StatusCode::OK, Json(model_to_json(&m))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "model_not_found", "model_id": id })),
        )
            .into_response(),
        Err(e) => internal_err("get_failed", e),
    }
}

/// `PATCH /v1/admin/models/{id}` — partial update.
///
/// Each field in the body is independently optional:
///
/// ```jsonc
/// {
///   "display_name":  "qwen3 8b q4",
///   "runtime":       { "n_gpu_layers": 999, "ctx_size": 8192 },
///   "defaults":      { "temperature": 0.7, "top_p": 0.9 },
///   "chat_template": { "format": "chatml" },
///   "stop_tokens":   ["</s>"],          // null clears
///   "system_prompt": "You are ...",     // null clears
///   "tags":          ["coder","int8"],  // null clears
///   "source_spec":   { "kind": "hf", ... }  // triggers re-install via swap_blob
/// }
/// ```
///
/// `family` and `quantization` are not patchable — those are identity.
/// `source_spec` requires the blob swap dance and currently returns 501
/// until the orchestrator wires `swap_blob` into the FSM. Other fields
/// take effect on the next ProcessManager start.
#[derive(Debug, Deserialize)]
pub struct PatchModelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<Option<String>>,
    #[serde(default)]
    pub source_spec: Option<serde_json::Value>,
    #[serde(default)]
    pub runtime: Option<serde_json::Value>,
    #[serde(default)]
    pub defaults: Option<serde_json::Value>,
    #[serde(default)]
    pub chat_template: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_tokens: Option<Option<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Option<Vec<String>>>,
}

#[instrument(skip_all)]

pub async fn patch_model(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PatchModelRequest>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };

    // source_spec requires re-install; defer to a future endpoint.
    if req.source_spec.is_some() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "source_spec_patch_not_supported",
                "message": "Changing source_spec triggers a re-install. Delete the model and re-register, or wait for the swap_blob endpoint.",
            })),
        )
            .into_response();
    }

    let patch = ModelfilePatch {
        display_name: req.display_name,
        source_spec: req.source_spec,
        runtime: req.runtime,
        defaults: req.defaults,
        chat_template: req.chat_template,
        stop_tokens: req.stop_tokens,
        system_prompt: req.system_prompt,
        tags: req.tags,
    };

    match registry.patch(&id, &patch).await {
        Ok(true) => match registry.get(&id).await {
            Ok(Some(m)) => (StatusCode::OK, Json(model_to_json(&m))).into_response(),
            // Race: row was deleted between PATCH and re-fetch. Surface as
            // 200 with just the id so the caller doesn't 500 on a successful
            // write.
            Ok(None) => (StatusCode::OK, Json(json!({ "model_id": id, "stale": true })))
                .into_response(),
            Err(e) => internal_err("get_after_patch_failed", e),
        },
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "model_not_found", "model_id": id })),
        )
            .into_response(),
        Err(e) => internal_err("patch_failed", e),
    }
}

#[instrument(skip_all)]

pub async fn delete_model(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };
    match registry.delete(&id).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "model_not_found", "model_id": id })),
        )
            .into_response(),
        Err(e) => internal_err("delete_failed", e),
    }
}

#[instrument(skip_all)]

pub async fn promote_model(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };
    match registry.promote_default(&id).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "model_id": id, "is_default": true })))
            .into_response(),
        Err(e) => internal_err("promote_failed", e),
    }
}

// ── Install endpoints ───────────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn list_attempts(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListAttemptsQuery>,
) -> impl IntoResponse {
    let Some(log) = state.install_attempts_log.as_ref() else {
        return not_wired().into_response();
    };
    let filter = AttemptsFilter {
        model_id: Some(id.clone()),
        limit: q.limit.or(Some(50)),
        offset: q.offset,
    };
    match log.list(&filter).await {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id,
                        "model_id": a.model_id,
                        "attempt_no": a.attempt_no,
                        "started_at": a.started_at,
                        "finished_at": a.finished_at,
                        "status": a.status.as_str(),
                        "stage": a.stage.map(|s| s.as_str()),
                        "error_kind": a.error_kind.map(|k| k.as_str()),
                        "error_message": a.error_message,
                        "bytes_downloaded": a.bytes_downloaded,
                        "total_bytes": a.total_bytes,
                        "duration_ms": a.duration_ms,
                        "retryable": a.retryable,
                        "triggered_by": a.triggered_by.map(|t| t.as_str()),
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "attempts": items }))).into_response()
        }
        Err(e) => internal_err("list_attempts_failed", e),
    }
}

/// `POST /v1/admin/models/:id/install/retry`. Phase 2 just opens a fresh
/// attempt row — actual orchestration awaits a follow-up commit that wires
/// the source-spec re-resolution path. The endpoint is exposed so the admin
/// UI can POST against it without code changes when wiring lands.
#[instrument(skip_all)]
pub async fn retry_install(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };
    let Some(log) = state.install_attempts_log.as_ref() else {
        return not_wired().into_response();
    };

    // Only allow retry from the failed terminal state.
    let model = match registry.get(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "model_not_found", "model_id": id })),
            )
                .into_response();
        }
        Err(e) => return internal_err("get_failed", e).into_response(),
    };
    if model.install_status != InstallStatus::Failed {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "not_failed",
                "install_status": model.install_status.as_str(),
                "message": "retry only allowed when install_status='failed'",
            })),
        )
            .into_response();
    }

    let attempt_id = match log.start(&id, TriggeredBy::AdminRetry).await {
        Ok(id) => id,
        Err(e) => return internal_err("start_attempt_failed", e).into_response(),
    };
    let _ = registry
        .update_install_status(
            &id,
            InstallStatus::Pending,
            None,
            None,
            Some(Utc::now()),
        )
        .await;
    (
        StatusCode::ACCEPTED,
        Json(json!({ "attempt_id": attempt_id, "model_id": id })),
    )
        .into_response()
}

#[instrument(skip_all)]

pub async fn cancel_install(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(log) = state.install_attempts_log.as_ref() else {
        return not_wired().into_response();
    };
    let Some(registry) = state.modelfile_registry.as_ref() else {
        return not_wired().into_response();
    };

    // Find the latest in_progress attempt for this model.
    let recent = match log
        .list(&AttemptsFilter {
            model_id: Some(id.clone()),
            limit: Some(5),
            offset: None,
        })
        .await
    {
        Ok(r) => r,
        Err(e) => return internal_err("list_attempts_failed", e).into_response(),
    };
    let in_progress = recent.iter().find(|a| {
        a.status == crate::domain::entities::AttemptStatus::InProgress
    });
    let attempt = match in_progress {
        Some(a) => a,
        None => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "no_in_progress_attempt",
                    "model_id": id,
                })),
            )
                .into_response();
        }
    };

    if let Err(e) = log.cancel(attempt.id).await {
        return internal_err("cancel_failed", e).into_response();
    }
    let _ = registry
        .update_install_status(
            &id,
            InstallStatus::Failed,
            Some(crate::domain::entities::ErrorKind::Unknown),
            Some("admin cancelled"),
            Some(Utc::now()),
        )
        .await;
    (
        StatusCode::OK,
        Json(json!({ "attempt_id": attempt.id, "model_id": id, "cancelled": true })),
    )
        .into_response()
}

/// `GET /v1/admin/models/:id/install/stream` — Server-Sent Events.
/// Subscribes to the install_orchestrator's broadcast channel for `id` and
/// re-emits every event as a `data:` line. Slow consumers see `Lagged` and
/// just miss intermediate progress events.
#[instrument(skip_all)]
pub async fn install_stream(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(orch) = state.install_orchestrator.as_ref() else {
        return not_wired().into_response();
    };

    let rx = orch.subscribe(&id).await;
    let stream: futures::stream::BoxStream<'static, Result<Event, Infallible>> = futures::stream::unfold((), move |_| {
        let mut rx = rx.resubscribe();
        async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let payload = serialize_event(&event);
                        return Some((Ok(Event::default().data(payload)), ()));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        }
    })
    .boxed();
    let _ = rx;

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
        .into_response()
}

fn serialize_event(event: &InstallEvent) -> String {
    match event {
        InstallEvent::StatusChanged { model_id, status } => json!({
            "event": "status_changed",
            "model_id": model_id,
            "install_status": status.as_str(),
        })
        .to_string(),
        InstallEvent::Progress { model_id, stage, bytes_done, total_bytes } => json!({
            "event": "progress",
            "model_id": model_id,
            "stage": stage.as_str(),
            "bytes_done": bytes_done,
            "total_bytes": total_bytes,
        })
        .to_string(),
        InstallEvent::Failed { model_id, error_kind, error_message } => json!({
            "event": "failed",
            "model_id": model_id,
            "error_kind": error_kind.as_str(),
            "error_message": error_message,
        })
        .to_string(),
        InstallEvent::Succeeded { model_id, sha256 } => json!({
            "event": "succeeded",
            "model_id": model_id,
            "sha256": sha256,
        })
        .to_string(),
    }
}

// ── Blob endpoints ──────────────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn list_orphan_blobs(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Query(q): Query<OrphanQuery>,
) -> impl IntoResponse {
    let Some(blobs) = state.blob_registry.as_ref() else {
        return not_wired().into_response();
    };

    let filter = OrphanFilter {
        min_orphan_days: q.min_orphan_days,
        limit: q.limit.or(Some(200)),
        offset: q.offset,
    };
    match blobs.list_orphans(&filter).await {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|b| {
                    json!({
                        "sha256": b.sha256,
                        "size_bytes": b.size_bytes,
                        "s3_key": b.s3_key,
                        "ref_count": b.ref_count,
                        "orphan_since": b.orphan_since,
                        "orphan_days": b.orphan_days(),
                        "source_history": b.source_history,
                        "created_at": b.created_at,
                        "last_accessed_at": b.last_accessed_at,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "blobs": items }))).into_response()
        }
        Err(e) => internal_err("list_orphans_failed", e),
    }
}

#[instrument(skip_all)]

pub async fn delete_blob(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(sha256): Path<String>,
) -> impl IntoResponse {
    let Some(blobs) = state.blob_registry.as_ref() else {
        return not_wired().into_response();
    };
    let Some(store) = state.blob_store.as_ref() else {
        return not_wired().into_response();
    };

    let blob = match blobs.get(&sha256).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "blob_not_found", "sha256": sha256 })),
            )
                .into_response();
        }
        Err(e) => return internal_err("get_blob_failed", e).into_response(),
    };
    if blob.ref_count > 0 {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "blob_referenced",
                "sha256": sha256,
                "ref_count": blob.ref_count,
                "message": "Delete the referencing Modelfile rows before removing the blob.",
            })),
        )
            .into_response();
    }
    if let Err(e) = store.delete(&sha256).await {
        return internal_err("garage_delete_failed", e).into_response();
    }
    if let Err(e) = blobs.delete(&sha256).await {
        return internal_err("registry_delete_failed", e).into_response();
    }
    (StatusCode::NO_CONTENT, ()).into_response()
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn internal_err(kind: &str, e: anyhow::Error) -> axum::response::Response {
    tracing::error!(kind, error = %e, "admin modelfile handler error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": kind, "message": e.to_string() })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_install_status_known_values() {
        assert_eq!(parse_install_status("ready"), Some(InstallStatus::Ready));
        assert_eq!(parse_install_status("failed"), Some(InstallStatus::Failed));
        assert_eq!(parse_install_status("downloading"), Some(InstallStatus::Downloading));
        assert_eq!(parse_install_status("nonsense"), None);
    }
}
