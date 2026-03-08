use async_trait::async_trait;
use uuid::Uuid;

/// Object storage port for LLM conversation contexts (`messages_json`).
///
/// The S3-compatible adapter stores each job's full context as
/// `messages/{job_id}.json`. PostgreSQL is no longer the primary
/// store for this field — `messages_json` in `inference_jobs` stays
/// NULL for new jobs; this port is authoritative.
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Upload conversation context for a job. Overwrites any existing object.
    async fn put(&self, job_id: Uuid, data: &serde_json::Value) -> anyhow::Result<()>;

    /// Fetch conversation context. Returns `None` if the object does not exist.
    async fn get(&self, job_id: Uuid) -> anyhow::Result<Option<serde_json::Value>>;
}
