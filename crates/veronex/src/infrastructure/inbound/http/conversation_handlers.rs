//! Conversation API handlers.
//!
//! GET /v1/conversations       — paginated conversation list
//! GET /v1/conversations/{id}  — conversation detail with turns

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::AppError;
use super::middleware::jwt_auth::RequireDashboardView;
use super::state::AppState;
use super::inference_helpers::to_public_id;
use crate::application::ports::outbound::message_store::ConversationRecord;

const CONV_CACHE_TTL_SECS: i64 = 300; // 5 min

fn conv_cache_key(conv_id: Uuid) -> String {
    format!("conv_s3:{}", conv_id)
}

/// Fetch ConversationRecord from Valkey cache; on miss, load from S3 and cache result.
async fn fetch_conv_s3_cached(
    state: &AppState,
    owner_id: Uuid,
    date: chrono::NaiveDate,
    conv_id: Uuid,
) -> Option<ConversationRecord> {
    let cache_key = conv_cache_key(conv_id);

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
}

fn default_limit() -> i64 { 50 }

#[derive(Serialize)]
pub struct ConversationSummary {
    id: Uuid,
    public_id: String,
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
    pub job_id: Uuid,
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
    id: Uuid,
    public_id: String,
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

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/conversations`
pub async fn list_conversations(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Query(params): Query<ListConversationsQuery>,
) -> HandlerResult<Json<ConversationListResponse>> {
    use sqlx::Row;

    let limit = params.limit.min(200);
    let offset = params.offset.max(0);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations")
        .fetch_one(&state.pg_pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count failed: {e}")))?;

    let rows = sqlx::query(
        "SELECT id, title, model_name, source, turn_count, total_prompt_tokens, total_completion_tokens, created_at, updated_at
         FROM conversations
         ORDER BY updated_at DESC
         LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("list failed: {e}")))?;

    let conversations = rows.iter().map(|r| {
        let id: Uuid = r.get("id");
        ConversationSummary {
            id,
            public_id: to_public_id(&id),
            title: r.get("title"),
            model_name: r.get("model_name"),
            source: r.get::<Option<String>, _>("source").unwrap_or_else(|| "api".to_string()),
            turn_count: r.get("turn_count"),
            total_prompt_tokens: r.get("total_prompt_tokens"),
            total_completion_tokens: r.get("total_completion_tokens"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }
    }).collect();

    Ok(Json(ConversationListResponse { conversations, total }))
}

/// `GET /v1/conversations/{id}`
///
/// Accepts base62 public_id (e.g. "32q9vHjvNhJXxVqI4WIuJ") or raw UUID.
pub async fn get_conversation(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> HandlerResult<Json<ConversationDetailResponse>> {
    use sqlx::Row;
    use super::inference_helpers::decode_conversation_id;

    // Accept both base62 and raw UUID
    let conv_id = decode_conversation_id(&id_str)
        .or_else(|| Uuid::parse_str(&id_str).ok())
        .ok_or_else(|| AppError::BadRequest("invalid conversation id".into()))?;

    // Fetch conversation metadata
    let conv_row = sqlx::query(
        "SELECT id, title, model_name, source, turn_count, total_prompt_tokens, total_completion_tokens, created_at, updated_at, account_id, api_key_id
         FROM conversations WHERE id = $1"
    )
    .bind(conv_id)
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("fetch failed: {e}")))?
    .ok_or_else(|| AppError::NotFound("conversation not found".into()))?;

    // Fetch full conversation record from S3 only (Valkey-cached, 5 min TTL).
    // No DB query for turns — S3 has the complete data.
    let owner_id: Uuid = conv_row.get::<Option<Uuid>, _>("account_id")
        .or_else(|| conv_row.get::<Option<Uuid>, _>("api_key_id"))
        .unwrap_or(conv_id);
    let date = conv_row.get::<DateTime<Utc>, _>("created_at").date_naive();
    let s3_record = fetch_conv_s3_cached(&state, owner_id, date, conv_id).await;

    let turns: Vec<ConversationTurn> = s3_record
        .map(|rec| rec.turns.into_iter().map(|t| ConversationTurn {
            job_id: t.job_id,
            prompt: t.prompt,
            result: t.result,
            tool_calls: t.tool_calls,
            model_name: t.model_name,
            created_at: t.created_at,
        }).collect())
        .unwrap_or_default();

    let id: Uuid = conv_row.get("id");
    Ok(Json(ConversationDetailResponse {
        id,
        public_id: to_public_id(&id),
        title: conv_row.get("title"),
        model_name: conv_row.get("model_name"),
        source: conv_row.get::<Option<String>, _>("source").unwrap_or_else(|| "api".to_string()),
        turn_count: conv_row.get("turn_count"),
        total_prompt_tokens: conv_row.get("total_prompt_tokens"),
        total_completion_tokens: conv_row.get("total_completion_tokens"),
        created_at: conv_row.get("created_at"),
        updated_at: conv_row.get("updated_at"),
        turns,
    }))
}
