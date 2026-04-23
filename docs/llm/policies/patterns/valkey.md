# Code Patterns: Rust — Valkey (Redis-compatible) Patterns

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Valkey Key Registry

All `veronex:*` key patterns MUST be defined in `infrastructure/outbound/valkey_keys.rs`.
This is the single source of truth — never hardcode key strings elsewhere.

## Valkey Lua Eval

Multi-step Valkey ops must be atomic. Single `EVAL` instead of multiple round-trips.

```rust
const RATE_LIMIT_SCRIPT: &str = r#"
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
"#;
let count: u64 = pool.next()
  .eval(RATE_LIMIT_SCRIPT, vec![key], vec![window_start, now_ms, member]).await?;
```

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

