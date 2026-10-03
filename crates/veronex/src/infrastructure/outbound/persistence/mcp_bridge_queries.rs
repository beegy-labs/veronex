//! SSOT for the SQL behind the MCP bridge ReAct flow. Mutations live
//! inline in the bridge today — this module centralises them so the
//! audit registry sees no raw SQL outside `persistence/`.

use sqlx::PgPool;
use uuid::Uuid;

/// SQL for the per-key `mcp_cap_points` cache lookup. Used as a string
/// argument to `cached_mcp_int_lookup` in `bridge.rs`.
pub const SELECT_MCP_CAP_POINTS: &str =
    "SELECT mcp_cap_points FROM api_keys WHERE id = $1";

/// SQL for the per-key minimum `top_k` cache lookup.
pub const SELECT_MIN_TOP_K_FOR_KEY: &str =
    "SELECT MIN(top_k) FROM mcp_key_access WHERE api_key_id = $1 \
     AND is_allowed = true AND top_k IS NOT NULL";

/// Roll up the prompt/completion token totals onto the surviving
/// "first" inference_jobs row at the end of a ReAct sequence.
pub async fn roll_up_first_job_tokens(
    pool: &PgPool,
    first_job_id: Uuid,
    prompt_tokens: i32,
    completion_tokens: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE inference_jobs SET prompt_tokens = $1, completion_tokens = $2 WHERE id = $3",
    )
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(first_job_id)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Delete the intermediate ReAct jobs that fed into the surviving
/// first job. Caller passes the id list; empty input is a no-op
/// (handled outside).
pub async fn delete_intermediate_jobs(pool: &PgPool, ids: &[Uuid]) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM inference_jobs WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Bump the conversation header for one completed ReAct turn.
pub async fn bump_conversation_turn(
    pool: &PgPool,
    conversation_id: Uuid,
    prompt_tokens: i32,
    completion_tokens: i32,
    model_name: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE conversations \
            SET turn_count = turn_count + 1, \
                total_prompt_tokens = total_prompt_tokens + $1, \
                total_completion_tokens = total_completion_tokens + $2, \
                model_name = COALESCE(model_name, $3), \
                updated_at = now() \
          WHERE id = $4",
    )
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(model_name)
    .bind(conversation_id)
    .execute(pool)
    .await
    .map(|_| ())
}
