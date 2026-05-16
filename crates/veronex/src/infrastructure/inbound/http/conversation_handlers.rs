#![allow(clippy::collapsible_if, clippy::manual_clamp)]

//! Conversation API handlers.
//!
//! GET /v1/conversations                                 — paginated conversation list
//! GET /v1/conversations/{id}                            — conversation detail with turns
//! GET /v1/conversations/{id}/turns/{job_id}/internals   — compression + vision metadata (admin)

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tracing::instrument;

use super::error::AppError;
use super::middleware::jwt_auth::{RequireAccountManage, RequireDashboardView};
use super::state::AppState;
use crate::application::ports::outbound::message_store::ConversationRecord;
use crate::domain::constants::CONV_CACHE_TTL_SECS;
use crate::domain::value_objects::{ConvId, JobId};
use crate::infrastructure::outbound::valkey_keys::conv_s3_cache;

/// Fetch ConversationRecord from Valkey cache; on miss, load from S3 and cache result.
async fn fetch_conv_s3_cached(
    state: &AppState,
    owner_id: Uuid,
    date: chrono::NaiveDate,
    conv_id: Uuid,
) -> Option<ConversationRecord> {
    let cache_key = conv_s3_cache(conv_id);

    // ── Try Valkey cache first ────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        if let Ok(Some(json)) = pool.get::<Option<String>, _>(&cache_key).await {
            if let Ok(record) = serde_json::from_str::<ConversationRecord>(&json) {
                return Some(record);
            }
        }
    }

    // ── Cache miss: load from S3 ──────────────────────────────────────────────
    let store = state.message_store.as_ref()?;
    let record = store.get_conversation(owner_id, date, conv_id).await.ok().flatten()?;

    // ── Write to Valkey ───────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        if let Ok(json) = serde_json::to_string(&record) {
            let _: Result<(), _> = pool
                .set(&cache_key, json, Some(Expiration::EX(CONV_CACHE_TTL_SECS)), None, false)
                .await;
        }
    }

    Some(record)
}


type HandlerResult<T> = Result<T, AppError>;

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListConversationsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    source: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 { 50 }

#[derive(Serialize)]
pub struct ConversationSummary {
    id: ConvId,
    title: Option<String>,
    model_name: Option<String>,
    source: String,
    turn_count: i32,
    total_prompt_tokens: i32,
    total_completion_tokens: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ConversationListResponse {
    conversations: Vec<ConversationSummary>,
    total: i64,
}

#[derive(Serialize)]
pub struct ConversationTurn {
    pub job_id: JobId,
    pub prompt: String,
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ConversationDetailResponse {
    id: ConvId,
    title: Option<String>,
    model_name: Option<String>,
    source: String,
    turn_count: i32,
    total_prompt_tokens: i32,
    total_completion_tokens: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    turns: Vec<ConversationTurn>,
}

// ── Internals response types (Phase 8) ───────────────────────────────────────

#[derive(Serialize)]
struct CompressedTurnDetail {
    summary:           String,
    original_tokens:   u32,
    compressed_tokens: u32,
    compression_model: String,
    ratio:             f32,
}

#[derive(Serialize)]
struct VisionAnalysisDetail {
    analysis:        String,
    vision_model:    String,
    image_count:     u32,
    analysis_tokens: u32,
}

#[derive(Serialize)]
struct ToolCallDetail {
    round:           i16,
    server_slug:     String,
    tool_name:       String,
    namespaced_name: String,
    args:            serde_json::Value,
    result_text:     Option<String>,
    outcome:         String,
    cache_hit:       bool,
    latency_ms:      Option<i32>,
    result_bytes:    Option<i32>,
    created_at:      DateTime<Utc>,
}

#[derive(Serialize)]
struct TurnInternalsResponse {
    job_id:          String,
    compressed:      Option<CompressedTurnDetail>,
    vision_analysis: Option<VisionAnalysisDetail>,
    /// MCP per-tool audit for this turn — joined from `mcp_loop_tool_calls`
    /// (CDD `inference/mcp-schema.md`). Empty Vec when no MCP tools were
    /// invoked. Ordered by `loop_round ASC, created_at ASC`.
    /// SDD: `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md`.
    tool_calls:      Vec<ToolCallDetail>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/conversations`
#[instrument(skip_all)]
pub async fn list_conversations(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Query(params): Query<ListConversationsQuery>,
) -> HandlerResult<Json<ConversationListResponse>> {
    use crate::infrastructure::outbound::persistence::conversation_queries as conv_q;

    let limit = params.limit.max(1).min(200);
    let offset = params.offset.max(0);
    let search_pat = params.search.as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));

    let total = conv_q::count(&state.pg_pool, params.source.as_deref(), search_pat.as_deref())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count failed: {e}")))?;

    let rows = conv_q::list_page(
        &state.pg_pool,
        params.source.as_deref(),
        search_pat.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("list failed: {e}")))?;

    let conversations = rows.into_iter().map(|r| ConversationSummary {
        id: ConvId::from_uuid(r.id),
        title: r.title,
        model_name: r.model_name,
        source: r.source.unwrap_or_else(|| "api".to_string()),
        turn_count: r.turn_count,
        total_prompt_tokens: r.total_prompt_tokens,
        total_completion_tokens: r.total_completion_tokens,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }).collect();

    Ok(Json(ConversationListResponse { conversations, total }))
}

/// `GET /v1/conversations/{id}`
///
/// Accepts `conv_{base62}` public ID (e.g. "conv_3X4aB...").
#[instrument(skip_all)]
pub async fn get_conversation(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> HandlerResult<Json<ConversationDetailResponse>> {
    use crate::infrastructure::outbound::persistence::conversation_queries as conv_q;

    let conv_id = id_str
        .parse::<ConvId>()
        .map(|c| c.0)
        .map_err(|_| AppError::BadRequest("invalid conversation id".into()))?;

    let conv = conv_q::get_detail(&state.pg_pool, conv_id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("fetch failed: {e}")))?
        .ok_or_else(|| AppError::NotFound("conversation not found".into()))?;

    // Full conversation record comes from S3 (Valkey-cached, 5 min TTL).
    let owner_id: Uuid = conv.account_id.or(conv.api_key_id).unwrap_or(conv_id);
    let date = conv.created_at.date_naive();
    let s3_record = fetch_conv_s3_cached(&state, owner_id, date, conv_id).await;

    let turns: Vec<ConversationTurn> = s3_record
        .map(|rec| rec.regular_turns().map(|t| ConversationTurn {
            job_id: JobId::from_uuid(t.job_id),
            prompt: t.prompt.clone(),
            result: t.result.clone(),
            tool_calls: t.tool_calls.clone(),
            model_name: t.model_name.clone(),
            created_at: t.created_at.clone(),
        }).collect())
        .unwrap_or_default();

    Ok(Json(ConversationDetailResponse {
        id: ConvId::from_uuid(conv.id),
        title: conv.title,
        model_name: conv.model_name,
        source: conv.source.unwrap_or_else(|| "api".to_string()),
        turn_count: conv.turn_count,
        total_prompt_tokens: conv.total_prompt_tokens,
        total_completion_tokens: conv.total_completion_tokens,
        created_at: conv.created_at,
        updated_at: conv.updated_at,
        turns,
    }))
}

// ── GET /v1/conversations/{id}/turns/{job_id}/internals ───────────────────────

/// `GET /v1/conversations/{id}/turns/{job_id}/internals`
///
/// Returns compression and vision analysis metadata for a single turn.
/// Requires `account_manage` permission (admin-only).
#[instrument(skip_all)]
pub async fn get_turn_internals(
    RequireAccountManage(_): RequireAccountManage,
    State(state): State<AppState>,
    Path((conv_id_str, job_id_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let conv_uuid = match conv_id_str.parse::<ConvId>().map(|c| c.0)
        .or_else(|_| Uuid::parse_str(&conv_id_str).map_err(|e| e.to_string()))
    {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid conversation id"}))).into_response(),
    };

    let job_uuid = match job_id_str.parse::<JobId>().map(|j| j.0)
        .or_else(|_| Uuid::parse_str(&job_id_str).map_err(|e| e.to_string()))
    {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid job id"}))).into_response(),
    };

    if state.message_store.is_none() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "message store not configured"}))).into_response();
    }

    use crate::infrastructure::outbound::persistence::conversation_queries as conv_q;
    let row = conv_q::first_job_owner(&state.pg_pool, conv_uuid).await;

    let (owner_id, date) = match row {
        Ok(Some(r)) => {
            let date = r.created_at.date_naive();
            let owner = r.account_id.or(r.api_key_id).unwrap_or(conv_uuid);
            (owner, date)
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "conversation not found"}))).into_response(),
        Err(e) => {
            tracing::error!("get_turn_internals db: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db error"}))).into_response();
        }
    };

    let record = match fetch_conv_s3_cached(&state, owner_id, date, conv_uuid).await {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "conversation record not found"}))).into_response(),
    };

    let turn = match record.regular_turns().find(|t| t.job_id == job_uuid) {
        Some(t) => t,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "turn not found"}))).into_response(),
    };

    let compressed = turn.compressed.as_ref().map(|c| CompressedTurnDetail {
        summary:           c.summary.clone(),
        original_tokens:   c.original_tokens,
        compressed_tokens: c.compressed_tokens,
        compression_model: c.compression_model.clone(),
        ratio:             c.original_tokens as f32 / c.compressed_tokens.max(1) as f32,
    });

    let vision_analysis = turn.vision_analysis.as_ref().map(|v| VisionAnalysisDetail {
        analysis:        v.analysis.clone(),
        vision_model:    v.vision_model.clone(),
        image_count:     v.image_count,
        analysis_tokens: v.analysis_tokens,
    });

    // MCP tool-call audit moved to S3 `ConversationRecord.turns[].tool_calls[]`
    // (see `bridge.rs::run_loop` consolidated turn write). The conversation
    // detail GET surfaces every round's args + result + outcome inline, so
    // this endpoint no longer carries `tool_calls`. Field retained as an
    // empty array for backwards-compatible TS clients.
    (StatusCode::OK, Json(TurnInternalsResponse {
        job_id: job_uuid.to_string(),
        compressed,
        vision_analysis,
        tool_calls: Vec::new(),
    })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conv_cache_key_format() {
        let id = uuid::Uuid::nil();
        let key = conv_s3_cache(id);
        assert!(key.starts_with("conv_s3:"));
        assert!(key.contains(&id.to_string()));
    }

    #[test]
    fn default_limit_is_50() {
        assert_eq!(default_limit(), 50);
    }
}
