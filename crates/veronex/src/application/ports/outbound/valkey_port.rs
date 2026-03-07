use anyhow::Result;
use uuid::Uuid;

use crate::domain::value_objects::JobStatusEvent;

/// Abstracts Valkey (Redis-compatible) operations used by the inference use case.
///
/// This keeps infrastructure types (`fred`) out of the application layer,
/// maintaining the hexagonal architecture boundary.
#[async_trait::async_trait]
pub trait ValkeyPort: Send + Sync {
    // ── Queue operations ────────────────────────────────────────────

    /// RPUSH a job ID to the end of a queue list.
    async fn queue_push(&self, queue_key: &str, job_id: Uuid) -> Result<()>;

    /// LPUSH a job ID to the front of a queue list (re-queue with priority).
    async fn queue_push_front(&self, queue_key: &str, job_id: Uuid) -> Result<()>;

    /// Atomic priority pop via Lua: tries queues in order, moves the popped
    /// value to a processing list. Returns `None` when all queues are empty.
    async fn queue_priority_pop(
        &self,
        source_queues: &[&str],
        processing_key: &str,
    ) -> Result<Option<String>>;

    /// LREM: remove one occurrence of a value from a list.
    async fn list_remove(&self, key: &str, value: &str) -> Result<()>;

    // ── Key-value operations ────────────────────────────────────────

    /// SET with EX (TTL in seconds). Optionally only-if-exists (XX flag).
    async fn kv_set(
        &self,
        key: &str,
        value: &str,
        ttl_secs: i64,
        only_if_exists: bool,
    ) -> Result<()>;

    /// GET a key; returns `None` when the key does not exist.
    async fn kv_get(&self, key: &str) -> Result<Option<String>>;

    /// DEL a key.
    async fn kv_del(&self, key: &str) -> Result<()>;

    // ── Counter operations ──────────────────────────────────────────

    /// INCRBY — atomically increment a counter by `delta` (may be negative).
    async fn incr_by(&self, key: &str, delta: i64) -> Result<i64>;

    // ── Pub/Sub ─────────────────────────────────────────────────────

    /// Publish a job status event to the cross-instance pub/sub channel.
    async fn publish_job_event(&self, event: &JobStatusEvent, instance_id: &str);

    /// Publish a cancel signal for a job via pub/sub.
    async fn publish_cancel(&self, job_id: Uuid);
}
