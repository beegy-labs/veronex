# Code Patterns: Rust — Valkey (Redis-compatible) Patterns

> SSOT | **Last Updated**: 2026-05-02 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Valkey Key Registry — two-layer SSOT

| Layer | Module | Returns | Caller |
|-------|--------|---------|--------|
| Canonical | `domain/constants.rs` | unprefixed `veronex:...` strings + builder fns (`job_owner_key`, `conversation_record_key`, …) | application code (only domain import allowed) |
| pk-aware shim | `infrastructure/outbound/valkey_keys.rs` | `pk(&domain::*_key())` | infrastructure that bypasses `ValkeyPort` and talks to fred directly |

`ValkeyAdapter` applies `pk()` automatically inside every key-taking method
(`kv_set`/`kv_get`/`kv_del`/`incr_by`/`queue_*`/`list_*`/`zset_claim` …) so
application code passes canonical keys and the deployment-time
`VALKEY_KEY_PREFIX` stays an infrastructure-boundary concern.

Never hardcode `"veronex:..."` strings outside these two modules.

## Valkey Lua: SCRIPT LOAD + EVALSHA, not inline EVAL

Multi-step Valkey ops must be atomic — use a single Lua script.
At fleet scale (1M+ TPS) the script body must NOT travel on every call;
load once at startup, send only the SHA1 thereafter.

```rust
use fred::types::scripts::Script;

const LUA_RATE_LIMIT: &str = r#"
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
"#;

// Build at construction; load once at startup via warmup().
let script = Script::from_lua(LUA_RATE_LIMIT);
script.load(pool.next()).await?;          // SCRIPT LOAD (boot)
let count: u64 = script.evalsha(&pool, vec![key], vec![window_start, now_ms, member]).await?;
```

Required Cargo features on `fred`: `i-scripts`, `sha-1`. See
`infrastructure/outbound/valkey_adapter.rs` for the canonical pattern
(`script_priority_pop`, `script_zset_enqueue`, `script_zset_claim`,
`script_zset_cancel` + `warmup()`).

## Job Counters — Valkey INCR/DECR

O(1) pending/running counts for dashboard. No DB polling in hot path.

| Key | Update | Read |
|-----|--------|------|
| `JOBS_PENDING_COUNTER` | INCR on submit, DECR on dispatch/cancel/fail | stats ticker GET |
| `JOBS_RUNNING_COUNTER` | INCR on dispatch, DECR on complete/fail/cancel | stats ticker GET |

| Safety | Detail |
|--------|--------|
| Double-DECR prevention | Check previous status before DECR |
| Startup reconciliation | DB COUNT → Valkey SET at boot |
| Periodic reconciliation | Every 60s: DB COUNT vs Valkey GET → SET if drift |
| Valkey unavailable | Fallback to DB query |

**TTL rule**: `heartbeat_ttl ≥ 3 × scrape_interval` — survives 2 missed cycles.

**Fallback**: when Valkey is absent, health_checker falls back to semaphore-limited (64) concurrent HTTP probes.

```rust
// Reading liveness — O(1) Valkey instead of N × HTTP
let keys: Vec<String> = active.iter().map(|p| valkey_keys::provider_heartbeat(p.id)).collect();
let values: Result<Vec<Option<String>>, _> = pool.mget(keys).await;
// Some(str) = online, None = TTL expired = offline
```

**Key format test**: `heartbeat::key()` is pure — test it to guard crate-boundary drift.

