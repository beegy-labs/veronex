use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Full conversation data stored in S3/MinIO as zstd-compressed JSON.
///
/// Key pattern: `conversations/{owner_id}/{YYYY-MM-DD}/{job_id}.json.zst`
///
/// Written once at inference completion. Read on-demand by the admin detail view.
/// Never written to Postgres — S3 is the single source of truth for conversation content.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// Full original prompt text (untruncated).
    pub prompt: String,
    /// Complete messages array: system prompt + history + current user message.
    /// None for simple /api/generate (prompt-only) requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    /// Structured tool/function calls returned by the model.
    /// Includes MCP tool calls and their results.
    /// None when the model produced a text-only response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    /// Final inference result text (full, untruncated).
    /// None for cancelled jobs or jobs that produced only tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// Object storage port for full LLM conversation records.
///
/// The S3 adapter stores each job's conversation as a zstd-compressed JSON object.
/// Postgres stores only lightweight metadata (`prompt_preview`, status, tokens, latency).
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Store the full conversation record for a job.
    ///
    /// - `owner_id`: account_id if JWT auth, api_key_id if key auth
    /// - `date`: job creation date (used for S3 partitioning)
    /// - `job_id`: UUIDv7 job identifier
    async fn put_conversation(
        &self,
        owner_id: Uuid,
        date: NaiveDate,
        job_id: Uuid,
        record: &ConversationRecord,
    ) -> anyhow::Result<()>;

    /// Retrieve a conversation record. Returns `None` if the object does not exist.
    async fn get_conversation(
        &self,
        owner_id: Uuid,
        date: NaiveDate,
        job_id: Uuid,
    ) -> anyhow::Result<Option<ConversationRecord>>;
}
