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

    /// DEL + return count: delete a list key entirely. Used to drain legacy queues.
    async fn list_drain(&self, key: &str) -> Result<u64>;

    // ── ZSET queue operations (Phase 3) ──────────────────────────────

    /// Atomic Lua: ZCARD guard + ZADD + INCR demand + HSET enqueue_at + HSET model.
    /// Returns `true` if enqueued, `false` if queue is full (429).
    async fn zset_enqueue(
        &self,
        job_id: Uuid,
        score: f64,
        model: &str,
        now_ms: u64,
        max_size: u64,
        max_per_model: u64,
    ) -> Result<bool>;

    /// ZRANGE 0..(k-1) WITHSCORES — peek top-K candidates without removing.
    async fn zset_peek(&self, k: u64) -> Result<Vec<(String, f64)>>;

    /// Atomic Lua: ZREM + RPUSH processing + DECR demand + HDEL enqueue_at + HDEL model.
    /// Returns `true` if claimed (ZREM returned 1), `false` if another instance won.
    async fn zset_claim(
        &self,
        job_id: &str,
        processing_key: &str,
        model: &str,
    ) -> Result<bool>;

    /// Atomic Lua: ZREM + DECR demand + HDEL enqueue_at + HDEL model.
    /// Returns `true` if removed from ZSET.
    async fn zset_cancel(&self, job_id: &str, model: &str) -> Result<bool>;

    /// ZCARD — current ZSET queue length.
    async fn zset_len(&self) -> Result<u64>;

    /// ZADD queue:active <deadline_ms> <job_id> — register new lease.
    async fn active_lease_set(&self, job_id: &str, deadline_ms: u64) -> Result<()>;

    /// ZADD XX queue:active <deadline_ms> <job_id> — renew existing lease.
    /// Returns true if the job is still in the active set (score updated), false if already removed.
    async fn active_lease_renew(&self, job_id: &str, deadline_ms: u64) -> Result<bool>;

    /// ZREM queue:active <job_id> — remove lease on job completion.
    async fn active_lease_remove(&self, job_id: &str) -> Result<()>;

    /// ZRANGEBYSCORE queue:active 0 now_ms — returns expired job_ids.
    async fn active_lease_expired(&self, now_ms: u64) -> Result<Vec<String>>;

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
