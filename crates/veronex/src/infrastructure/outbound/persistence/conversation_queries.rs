//! SSOT for the SQL behind the dashboard `/v1/conversations` endpoints.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ConversationListRow {
    pub id: Uuid,
    pub title: Option<String>,
    pub model_name: Option<String>,
    pub source: Option<String>,
    pub turn_count: i32,
    pub total_prompt_tokens: i32,
    pub total_completion_tokens: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ConversationDetailRow {
    pub id: Uuid,
    pub title: Option<String>,
    pub model_name: Option<String>,
    pub source: Option<String>,
    pub turn_count: i32,
    pub total_prompt_tokens: i32,
    pub total_completion_tokens: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub account_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
}

/// First-job ownership snapshot used by `get_turn_internals` to figure
/// out the S3 partition key.
#[derive(Debug, Clone)]
pub struct ConversationOwner {
    pub account_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const LIST_COLS: &str =
    "id, title, model_name, source, turn_count, total_prompt_tokens, \
     total_completion_tokens, created_at, updated_at";
const DETAIL_COLS: &str =
    "id, title, model_name, source, turn_count, total_prompt_tokens, \
     total_completion_tokens, created_at, updated_at, account_id, api_key_id";

const FILTER_PREDICATE: &str =
    "($1::text IS NULL OR source = $1) AND ($2::text IS NULL OR LOWER(title) LIKE $2)";

/// Total matching the same predicate the list query uses.
pub async fn count(
    pool: &PgPool,
    source: Option<&str>,
    search_pat: Option<&str>,
) -> sqlx::Result<i64> {
    let q = format!("SELECT COUNT(*) FROM conversations WHERE {FILTER_PREDICATE}");
    sqlx::query_scalar(&q)
        .bind(source)
        .bind(search_pat)
        .fetch_one(pool)
        .await
}

/// Paginated list, newest update first.
pub async fn list_page(
    pool: &PgPool,
    source: Option<&str>,
    search_pat: Option<&str>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<ConversationListRow>> {
    let q = format!(
        "SELECT {LIST_COLS} FROM conversations \
         WHERE {FILTER_PREDICATE} \
         ORDER BY updated_at DESC LIMIT $3 OFFSET $4"
    );
    let rows = sqlx::query(&q)
        .bind(source)
        .bind(search_pat)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|r| {
            Ok(ConversationListRow {
                id: r.try_get("id")?,
                title: r.try_get("title")?,
                model_name: r.try_get("model_name")?,
                source: r.try_get("source")?,
                turn_count: r.try_get("turn_count")?,
                total_prompt_tokens: r.try_get("total_prompt_tokens")?,
                total_completion_tokens: r.try_get("total_completion_tokens")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect()
}

/// Single-conversation detail row.
pub async fn get_detail(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<ConversationDetailRow>> {
    let q = format!("SELECT {DETAIL_COLS} FROM conversations WHERE id = $1");
    let row = sqlx::query(&q).bind(id).fetch_optional(pool).await?;
    row.map(|r| {
        Ok(ConversationDetailRow {
            id: r.try_get("id")?,
            title: r.try_get("title")?,
            model_name: r.try_get("model_name")?,
            source: r.try_get("source")?,
            turn_count: r.try_get("turn_count")?,
            total_prompt_tokens: r.try_get("total_prompt_tokens")?,
            total_completion_tokens: r.try_get("total_completion_tokens")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
            account_id: r.try_get("account_id")?,
            api_key_id: r.try_get("api_key_id")?,
        })
    })
    .transpose()
}

/// Idempotent insert of a conversation header. Caller passes ownership
/// fields; subsequent calls for the same `id` are no-ops.
pub async fn upsert_header(
    pool: &PgPool,
    id: Uuid,
    account_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    title: Option<&str>,
    source: &str,
) -> sqlx::Result<sqlx::postgres::PgQueryResult> {
    sqlx::query(
        "INSERT INTO conversations (id, account_id, api_key_id, title, source, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, now(), now()) ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(account_id)
    .bind(api_key_id)
    .bind(title)
    .bind(source)
    .execute(pool)
    .await
}

/// Earliest-job ownership for a conversation — drives the S3 partition lookup.
pub async fn first_job_owner(
    pool: &PgPool,
    conv_id: Uuid,
) -> sqlx::Result<Option<ConversationOwner>> {
    let row = sqlx::query(
        "SELECT account_id, api_key_id, created_at \
         FROM inference_jobs \
         WHERE conversation_id = $1 \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(conv_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(ConversationOwner {
            account_id: r.try_get("account_id")?,
            api_key_id: r.try_get("api_key_id")?,
            created_at: r.try_get("created_at")?,
        })
    })
    .transpose()
}
