use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Single turn within a conversation stored in S3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub job_id: Uuid,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub created_at: String,
}

/// Full conversation data stored in S3/MinIO as zstd-compressed JSON.
///
/// Key pattern: `conversations/{owner_id}/{conversation_id}.json.zst`
///
/// One S3 object per conversation. Each turn appends to the `turns` array.
/// Postgres stores only lightweight metadata (preview, tokens, latency).
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// All turns in this conversation, ordered by creation time.
    pub turns: Vec<TurnRecord>,
}

impl ConversationRecord {
    pub fn new() -> Self {
        Self { turns: Vec::new() }
    }

    /// Get result for a specific job_id.
    pub fn result_for_job(&self, job_id: Uuid) -> Option<&str> {
        self.turns.iter()
            .find(|t| t.job_id == job_id)
            .and_then(|t| t.result.as_deref())
    }

    /// Get the latest turn's result.
    pub fn latest_result(&self) -> Option<&str> {
        self.turns.last().and_then(|t| t.result.as_deref())
    }
}

/// Object storage port for full LLM conversation records.
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Store the full conversation record.
    async fn put_conversation(
        &self,
        owner_id: Uuid,
        date: NaiveDate,
        conversation_id: Uuid,
        record: &ConversationRecord,
    ) -> anyhow::Result<()>;

    /// Retrieve a conversation record. Returns `None` if the object does not exist.
    async fn get_conversation(
        &self,
        owner_id: Uuid,
        date: NaiveDate,
        conversation_id: Uuid,
    ) -> anyhow::Result<Option<ConversationRecord>>;
}
