//! Distributed VRAM pool backed by Valkey HASH + ZSET.
//!
//! Uses the local `VramPool` for fast sync reserve (per-instance)
//! and publishes state to Valkey asynchronously for cross-instance
//! visibility and crash recovery.

use std::sync::Arc;

use fred::prelude::*;
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::{
    ModelVramProfile, VramPermit, VramPoolPort,
};
use crate::infrastructure::outbound::capacity::vram_pool::VramPool;

/// Publish VRAM reservation to Valkey: HINCRBY + ZADD lease.
/// ZSET member format: "instance_id:lease_id:kv_mb" (kv_mb for reaper deduction).
const LUA_VRAM_ACQUIRE: &str = r#"
local t = redis.call('TIME')
local now = tonumber(t[1])
local expiry = now + tonumber(ARGV[3])
local kv = tonumber(ARGV[4])
redis.call('HINCRBY', KEYS[1], ARGV[1], kv)
redis.call('ZADD', KEYS[2], expiry, ARGV[1] .. ':' .. ARGV[2] .. ':' .. kv)
return 1
"#;

/// Release VRAM in Valkey: HINCRBY negative, ZREM lease.
const LUA_VRAM_RELEASE: &str = r#"
local cur = tonumber(redis.call('HGET', KEYS[1], ARGV[1]) or '0')
local delta = tonumber(ARGV[3])
if cur >= delta then
    redis.call('HINCRBY', KEYS[1], ARGV[1], -delta)
else
    redis.call('HSET', KEYS[1], ARGV[1], 0)
end
redis.call('ZREM', KEYS[2], ARGV[1] .. ':' .. ARGV[2] .. ':' .. delta)
return 1
"#;

/// Reap expired leases: ZREM + HINCRBY deduction from reserved HASH.
const LUA_VRAM_REAP: &str = r#"
local t = redis.call('TIME')
local now = tonumber(t[1])
local expired = redis.call('ZRANGEBYSCORE', KEYS[2], '-inf', now)
local count = 0
for _, member in ipairs(expired) do
    redis.call('ZREM', KEYS[2], member)
    -- Extract instance_id and kv_mb from "instance_id:lease_id:kv_mb"
    local parts = {}
    for p in member:gmatch('[^:]+') do parts[#parts+1] = p end
    if #parts >= 3 then
        local inst = parts[1]
        local kv = tonumber(parts[#parts]) or 0
        if kv > 0 then
            local cur = tonumber(redis.call('HGET', KEYS[1], inst) or '0')
            if cur >= kv then
                redis.call('HINCRBY', KEYS[1], inst, -kv)
            else
                redis.call('HSET', KEYS[1], inst, 0)
            end
        end
    end
    count = count + 1
end
return count
"#;

const LEASE_DURATION_SECS: u32 = 120;

#[derive(Clone)]
pub struct DistributedVramPool {
    pool: Pool,
    instance_id: Arc<str>,
    local: VramPool,
}

impl DistributedVramPool {
    pub fn new(pool: Pool, instance_id: Arc<str>) -> Self {
        Self {
            pool,
            instance_id,
            local: VramPool::new(),
        }
    }

    /// Reap expired VRAM leases across all providers.
    pub async fn reap_all_expired(&self) {
        use fred::types::scan::Scanner as _;
        use futures::TryStreamExt as _;

        let mut keys: Vec<String> = Vec::new();
        let mut scanner = self.pool.next().scan("veronex:vram_leases:*", Some(100), None);
        while let Ok(Some(mut page)) = scanner.try_next().await {
            if let Some(results) = page.take_results() {
                for key in results.into_iter() {
                    let s: String = match key.convert() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    keys.push(s);
                }
            }
            page.next();
        }

        for leases_key in keys {
            let vram_key = leases_key.replace("vram_leases", "vram_reserved");
            let result: Result<u64, _> = self
                .pool
                .eval(
                    LUA_VRAM_REAP,
                    vec![vram_key, leases_key.clone()],
                    Vec::<String>::new(),
                )
                .await;
            match result {
                Ok(count) if count > 0 => {
                    tracing::info!(count, leases_key, "reaped expired VRAM leases");
                }
                Err(e) => {
                    tracing::warn!(leases_key, "VRAM lease reap failed: {e}");
                }
                _ => {}
            }
        }
    }
}

impl VramPoolPort for DistributedVramPool {
    fn try_reserve(&self, provider_id: Uuid, model: &str) -> Option<VramPermit> {
        // Local reserve for per-instance VRAM control (sync, O(1)).
        let local_permit = self.local.try_reserve(provider_id, model)?;
        let (reserved_kv, active_count, kv_mb) = local_permit.into_parts()?;

        // Publish reservation to Valkey async.
        let vram_key = format!("veronex:vram_reserved:{provider_id}");
        let leases_key = format!("veronex:vram_leases:{provider_id}");
        let lease_id = Uuid::new_v4().to_string();
        let instance_id = self.instance_id.to_string();

        let pool = self.pool.clone();
        let vk = vram_key.clone();
        let lk = leases_key.clone();
        let iid = instance_id.clone();
        let lid = lease_id.clone();
        let kv = kv_mb;
        tokio::spawn(async move {
            let _: Result<i64, _> = pool
                .eval(
                    LUA_VRAM_ACQUIRE,
                    vec![vk, lk],
                    vec![iid, lid, LEASE_DURATION_SECS.to_string(), kv.to_string()],
                )
                .await;
        });

        // On drop: decrement local + async release Valkey lease.
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<u32>();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            if let Ok(released_kv) = release_rx.await {
                let _: Result<i64, _> = pool
                    .eval(
                        LUA_VRAM_RELEASE,
                        vec![vram_key, leases_key],
                        vec![instance_id, lease_id, released_kv.to_string()],
                    )
                    .await;
            }
        });

        Some(VramPermit::combined(kv_mb, reserved_kv, active_count, release_tx))
    }

    fn total_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.local.total_vram_mb(provider_id)
    }

    fn used_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.local.used_vram_mb(provider_id)
    }

    fn available_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.local.available_vram_mb(provider_id)
    }

    fn set_total_vram(&self, provider_id: Uuid, total_mb: u32) {
        self.local.set_total_vram(provider_id, total_mb);
    }

    fn set_model_profile(&self, provider_id: Uuid, model: &str, profile: ModelVramProfile) {
        self.local.set_model_profile(provider_id, model, profile);
    }

    fn mark_model_loaded(&self, provider_id: Uuid, model: &str, weight_mb: u32) {
        self.local.mark_model_loaded(provider_id, model, weight_mb);
    }

    fn mark_model_unloaded(&self, provider_id: Uuid, model: &str) {
        self.local.mark_model_unloaded(provider_id, model);
    }

    fn active_requests(&self, provider_id: Uuid, model: &str) -> u32 {
        self.local.active_requests(provider_id, model)
    }

    fn provider_active_requests(&self, provider_id: Uuid) -> u32 {
        self.local.provider_active_requests(provider_id)
    }

    fn loaded_model_names(&self, provider_id: Uuid) -> Vec<String> {
        self.local.loaded_model_names(provider_id)
    }

    fn set_max_concurrent(&self, provider_id: Uuid, model: &str, limit: u32) {
        self.local.set_max_concurrent(provider_id, model, limit);
    }

    fn max_concurrent(&self, provider_id: Uuid, model: &str) -> u32 {
        self.local.max_concurrent(provider_id, model)
    }

    fn set_baseline_tps(&self, provider_id: Uuid, model: &str, tps_x100: u32) {
        self.local.set_baseline_tps(provider_id, model, tps_x100);
    }

    fn baseline_tps(&self, provider_id: Uuid, model: &str) -> u32 {
        self.local.baseline_tps(provider_id, model)
    }

    fn set_baseline_p95_ms(&self, provider_id: Uuid, model: &str, p95_ms: u32) {
        self.local.set_baseline_p95_ms(provider_id, model, p95_ms);
    }

    fn baseline_p95_ms(&self, provider_id: Uuid, model: &str) -> u32 {
        self.local.baseline_p95_ms(provider_id, model)
    }

    fn set_probe_config(&self, permits: i32, rate: i32) {
        self.local.set_probe_config(permits, rate);
    }
}
