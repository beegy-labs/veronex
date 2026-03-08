//! `ValkeyPort` adapter backed by `fred::clients::Pool`.
//!
//! All Valkey I/O flows through this adapter, keeping the `fred` crate
//! out of the application layer.

use anyhow::Result;
use fred::prelude::*;
use uuid::Uuid;

use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::value_objects::JobStatusEvent;
use crate::infrastructure::outbound::pubsub::relay;

/// Lua script: priority pop from N source queues into a processing list.
///
/// Tries each source queue in order (LMOVE LEFT → RIGHT).
/// Returns the popped value or `false` (nil) when all queues are empty.
const LUA_PRIORITY_POP: &str = r#"
for i = 1, #KEYS - 1 do
    local val = redis.call('LMOVE', KEYS[i], KEYS[#KEYS], 'LEFT', 'RIGHT')
    if val then return val end
end
return false
"#;

pub struct ValkeyAdapter {
    pool: Pool,
}

impl ValkeyAdapter {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Expose the inner pool for infrastructure code that still needs direct access
    /// (e.g. `SubscriberClient` setup, reaper, health checker).
    pub fn inner_pool(&self) -> &Pool {
        &self.pool
    }
}

#[async_trait::async_trait]
impl ValkeyPort for ValkeyAdapter {
    // ── Queue operations ────────────────────────────────────────────

    async fn queue_push(&self, queue_key: &str, job_id: Uuid) -> Result<()> {
        self.pool
            .rpush::<i64, _, _>(queue_key, job_id.to_string())
            .await?;
        Ok(())
    }

    async fn queue_push_front(&self, queue_key: &str, job_id: Uuid) -> Result<()> {
        self.pool
            .lpush::<i64, _, _>(queue_key, job_id.to_string())
            .await?;
        Ok(())
    }

    async fn queue_priority_pop(
        &self,
        source_queues: &[&str],
        processing_key: &str,
    ) -> Result<Option<String>> {
        let mut keys: Vec<String> = source_queues.iter().map(|s| s.to_string()).collect();
        keys.push(processing_key.to_string());

        let result: Option<String> = self
            .pool
            .eval(LUA_PRIORITY_POP, keys, Vec::<String>::new())
            .await?;
        Ok(result)
    }

    async fn list_remove(&self, key: &str, value: &str) -> Result<()> {
        self.pool.lrem::<i64, _, _>(key, 1, value).await?;
        Ok(())
    }

    // ── Key-value operations ────────────────────────────────────────

    async fn kv_set(
        &self,
        key: &str,
        value: &str,
        ttl_secs: i64,
        only_if_exists: bool,
    ) -> Result<()> {
        let set_opts = if only_if_exists {
            Some(SetOptions::XX)
        } else {
            None
        };
        self.pool
            .set::<(), _, _>(key, value, Some(Expiration::EX(ttl_secs)), set_opts, false)
            .await?;
        Ok(())
    }

    async fn kv_get(&self, key: &str) -> Result<Option<String>> {
        let result: Option<String> = self.pool.get(key).await?;
        Ok(result)
    }

    async fn kv_del(&self, key: &str) -> Result<()> {
        self.pool.del::<i64, _>(key).await?;
        Ok(())
    }

    // ── Counter operations ──────────────────────────────────────────

    async fn incr_by(&self, key: &str, delta: i64) -> Result<i64> {
        let result: i64 = self.pool.incr_by(key, delta).await?;
        Ok(result)
    }

    // ── Pub/Sub ─────────────────────────────────────────────────────

    async fn publish_job_event(&self, event: &JobStatusEvent, instance_id: &str) {
        relay::publish_job_event(&self.pool, event, instance_id).await;
    }

    async fn publish_cancel(&self, job_id: Uuid) {
        relay::publish_cancel(&self.pool, job_id).await;
    }
}
