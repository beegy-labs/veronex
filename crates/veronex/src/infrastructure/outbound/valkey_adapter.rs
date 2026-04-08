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

// ── ZSET queue Lua scripts (Phase 3) ──────────────────────────────────────

/// Atomic enqueue: ZCARD guard + per-model demand guard + ZADD + INCR + HSET×2.
/// KEYS[1]=queue:zset  KEYS[2]=demand:{model}  KEYS[3]=queue:enqueue_at  KEYS[4]=queue:model
/// ARGV[1]=job_id  ARGV[2]=score  ARGV[3]=max_size  ARGV[4]=now_ms  ARGV[5]=model  ARGV[6]=max_per_model
/// Returns: 1=ok, 0=global full, -1=per-model full
const LUA_ZSET_ENQUEUE: &str = r#"
if redis.call('ZCARD', KEYS[1]) >= tonumber(ARGV[3]) then return 0 end
local demand = tonumber(redis.call('GET', KEYS[2]) or '0')
if demand >= tonumber(ARGV[6]) then return -1 end
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
redis.call('INCR', KEYS[2])
redis.call('HSET', KEYS[3], ARGV[1], ARGV[4])
redis.call('HSET', KEYS[4], ARGV[1], ARGV[5])
return 1
"#;

/// Atomic claim: ZREM + ZADD active (with deadline) + DECR demand + HDEL side hashes.
/// KEYS[1]=queue:zset  KEYS[2]=queue:active  KEYS[3]=demand:{model}
/// KEYS[4]=queue:enqueue_at  KEYS[5]=queue:model
/// ARGV[1]=job_id  ARGV[2]=deadline_ms
/// Returns: 1=claimed, 0=already taken
const LUA_ZSET_CLAIM: &str = r#"
if redis.call('ZREM', KEYS[1], ARGV[1]) == 0 then return 0 end
redis.call('ZADD', KEYS[2], ARGV[2], ARGV[1])
local v = redis.call('DECR', KEYS[3])
if v < 0 then redis.call('SET', KEYS[3], 0) end
redis.call('HDEL', KEYS[4], ARGV[1])
redis.call('HDEL', KEYS[5], ARGV[1])
return 1
"#;

/// Atomic cancel from ZSET: ZREM + DECR demand + HDEL side hashes.
/// KEYS[1]=queue:zset  KEYS[2]=demand:{model}  KEYS[3]=queue:enqueue_at  KEYS[4]=queue:model
/// ARGV[1]=job_id
/// Returns: 1=removed, 0=not in ZSET
const LUA_ZSET_CANCEL: &str = r#"
if redis.call('ZREM', KEYS[1], ARGV[1]) == 0 then return 0 end
local v = redis.call('DECR', KEYS[2])
if v < 0 then redis.call('SET', KEYS[2], 0) end
redis.call('HDEL', KEYS[3], ARGV[1])
redis.call('HDEL', KEYS[4], ARGV[1])
return 1
"#;

pub struct ValkeyAdapter {
    pool: Pool,
}

impl ValkeyAdapter {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
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

    async fn list_drain(&self, key: &str) -> Result<u64> {
        let len: u64 = self.pool.llen(key).await.unwrap_or(0);
        if len > 0 {
            self.pool.del::<i64, _>(key).await?;
        }
        Ok(len)
    }

    // ── ZSET queue operations (Phase 3) ──────────────────────────────

    async fn zset_enqueue(
        &self,
        job_id: Uuid,
        score: f64,
        model: &str,
        now_ms: u64,
        max_size: u64,
        max_per_model: u64,
    ) -> Result<bool> {
        use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP};

        let demand_key = crate::domain::constants::demand_key(model);
        let keys = vec![
            QUEUE_ZSET.to_string(),
            demand_key,
            QUEUE_ENQUEUE_AT.to_string(),
            QUEUE_MODEL_MAP.to_string(),
        ];
        let args = vec![
            job_id.to_string(),
            score.to_string(),
            max_size.to_string(),
            now_ms.to_string(),
            model.to_string(),
            max_per_model.to_string(),
        ];

        let result: i64 = self.pool.eval(LUA_ZSET_ENQUEUE, keys, args).await?;
        if result == -1 {
            tracing::warn!(%job_id, %model, "per-model queue limit reached");
        }
        Ok(result == 1)
    }

    async fn zset_peek(&self, k: u64) -> Result<Vec<(String, f64)>> {
        use crate::domain::constants::QUEUE_ZSET;

        let raw: Vec<(String, f64)> = self
            .pool
            .zrange(QUEUE_ZSET, 0, (k as i64) - 1, None, false, None, true)
            .await?;
        Ok(raw)
    }

    async fn zset_claim(
        &self,
        job_id: &str,
        processing_key: &str,
        model: &str,
    ) -> Result<bool> {
        use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP, LEASE_TTL_MS};

        let deadline_ms = (chrono::Utc::now().timestamp_millis() as u64) + LEASE_TTL_MS;
        let demand_key = crate::domain::constants::demand_key(model);
        let keys = vec![
            QUEUE_ZSET.to_string(),
            processing_key.to_string(),
            demand_key,
            QUEUE_ENQUEUE_AT.to_string(),
            QUEUE_MODEL_MAP.to_string(),
        ];
        let args = vec![job_id.to_string(), deadline_ms.to_string()];

        let result: i64 = self.pool.eval(LUA_ZSET_CLAIM, keys, args).await?;
        Ok(result == 1)
    }

    async fn zset_cancel(&self, job_id: &str, model: &str) -> Result<bool> {
        use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP};

        let demand_key = crate::domain::constants::demand_key(model);
        let keys = vec![
            QUEUE_ZSET.to_string(),
            demand_key,
            QUEUE_ENQUEUE_AT.to_string(),
            QUEUE_MODEL_MAP.to_string(),
        ];
        let args = vec![job_id.to_string()];

        let result: i64 = self.pool.eval(LUA_ZSET_CANCEL, keys, args).await?;
        Ok(result == 1)
    }

    async fn zset_len(&self) -> Result<u64> {
        use crate::domain::constants::QUEUE_ZSET;
        let len: u64 = self.pool.zcard(QUEUE_ZSET).await?;
        Ok(len)
    }

    async fn active_lease_set(&self, job_id: &str, deadline_ms: u64) -> Result<()> {
        use crate::domain::constants::QUEUE_ACTIVE;
        let _: i64 = self.pool
            .zadd(QUEUE_ACTIVE, None, None, false, false, (deadline_ms as f64, job_id))
            .await?;
        Ok(())
    }

    async fn active_lease_renew(&self, job_id: &str, deadline_ms: u64) -> Result<bool> {
        use crate::domain::constants::QUEUE_ACTIVE;
        // ZADD XX updates score only if member already exists; returns 0 added (not changed).
        let _: i64 = self.pool
            .zadd(
                QUEUE_ACTIVE,
                Some(SetOptions::XX),
                None,
                false,
                false,
                (deadline_ms as f64, job_id),
            )
            .await
            .unwrap_or(0);
        // Check if the member still exists (ZADD XX does not report updates via return value)
        let score: Option<f64> = self.pool.zscore(QUEUE_ACTIVE, job_id).await.unwrap_or(None);
        Ok(score.is_some())
    }

    async fn active_lease_remove(&self, job_id: &str) -> Result<()> {
        use crate::domain::constants::QUEUE_ACTIVE;
        let _: i64 = self.pool.zrem(QUEUE_ACTIVE, job_id).await?;
        Ok(())
    }

    async fn active_lease_expired(&self, now_ms: u64) -> Result<Vec<String>> {
        use crate::domain::constants::QUEUE_ACTIVE;
        let members: Vec<fred::types::Value> = self.pool
            .zrangebyscore(QUEUE_ACTIVE, 0.0_f64, now_ms as f64, false, None)
            .await?;
        Ok(members.into_iter().filter_map(|v: fred::types::Value| v.as_string()).collect())
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
