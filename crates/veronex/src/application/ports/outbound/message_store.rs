use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Compression ──────────────────────────────────────────────────────────────

/// Per-turn compression output stored alongside the raw turn in S3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedTurn {
    /// Compressed Q&A summary. Target: ~100 tokens.
    pub summary: String,
    /// Token count of (prompt + result) before compression.
    pub original_tokens: u32,
    /// Token count of summary after compression.
    pub compressed_tokens: u32,
    /// Model used for compression (e.g. "qwen2.5:3b").
    pub compression_model: String,
}

// ── Vision ───────────────────────────────────────────────────────────────────

/// Result of the vision pre-processing call for an image-bearing turn.
///
/// Stored in `TurnRecord` before inference runs so that future compression
/// can preserve the image context as text rather than losing it entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionAnalysis {
    /// Image analysis text produced by the vision model (~200 tokens).
    pub analysis: String,
    /// Model used for analysis (e.g. "llava:7b").
    pub vision_model: String,
    /// Number of images analyzed.
    pub image_count: u32,
    /// Token count of the analysis output.
    pub analysis_tokens: u32,
}

// ── Session handoff ──────────────────────────────────────────────────────────

/// Master summary turn injected as the first turn of a new conversation
/// when the previous session is handed off due to context budget exhaustion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffTurn {
    /// Full-session master summary (~300 tokens).
    pub master_summary: String,
    /// Model used to produce the summary.
    pub summary_model: String,
    /// The conversation_id of the session that was handed off.
    pub previous_conversation_id: Uuid,
    /// Total turn count of the previous session.
    pub previous_turn_count: u32,
    pub created_at: String,
}

/// A turn in a conversation — either a regular inference turn or a handoff summary.
///
/// Uses untagged deserialization so that existing S3 records (plain `TurnRecord` JSON)
/// continue to deserialize correctly as `ConversationTurn::Regular`.
/// The `Handoff` variant is tried first; since `HandoffTurn` requires `master_summary`
/// and `TurnRecord` requires `job_id`, untagged matching is unambiguous.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConversationTurn {
    Handoff(HandoffTurn),
    Regular(TurnRecord),
}

impl ConversationTurn {
    pub fn as_regular(&self) -> Option<&TurnRecord> {
        match self {
            ConversationTurn::Regular(t) => Some(t),
            ConversationTurn::Handoff(_) => None,
        }
    }

    pub fn as_regular_mut(&mut self) -> Option<&mut TurnRecord> {
        match self {
            ConversationTurn::Regular(t) => Some(t),
            ConversationTurn::Handoff(_) => None,
        }
    }
}

// ── Turn record ──────────────────────────────────────────────────────────────

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub created_at: String,
    /// Per-turn compression output. `None` until compression completes asynchronously.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed: Option<CompressedTurn>,
    /// Vision pre-processing output for image-bearing turns.
    /// Written before inference runs; used during compression to preserve image context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_analysis: Option<VisionAnalysis>,
}

// ── Conversation record ──────────────────────────────────────────────────────

/// Full conversation data stored in S3/MinIO as zstd-compressed JSON.
///
/// Key pattern: `conversations/{owner_id}/{conversation_id}.json.zst`
///
/// One S3 object per conversation. Each turn appends to the `turns` array.
/// Postgres stores only lightweight metadata (preview, tokens, latency).
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// All turns in this conversation, ordered by creation time.
    /// May contain a leading `ConversationTurn::Handoff` when this is a
    /// renewed session following a context handoff.
    pub turns: Vec<ConversationTurn>,
}

impl ConversationRecord {
    pub fn new() -> Self {
        Self { turns: Vec::new() }
    }

    /// Regular (non-handoff) turns only.
    pub fn regular_turns(&self) -> impl Iterator<Item = &TurnRecord> {
        self.turns.iter().filter_map(|t| t.as_regular())
    }
}

// ── Port ─────────────────────────────────────────────────────────────────────

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
