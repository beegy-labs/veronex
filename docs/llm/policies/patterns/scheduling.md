# Code Patterns: Rust — Scheduling, Liveness & Scale

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Domain Services

Pure functions in `domain/services/` — no I/O, no async:

| Service | Function | Purpose |
|---------|----------|---------|
| `password_hashing` | `hash_password(password) → Result<String>` | Argon2id hashing (hexagonal: infra calls domain, not the reverse) |
| `message_hashing` | `hash_messages(msgs) → String` | SHA-256 content hash for deduplication |

## Timeout & TTL Constants

All timeouts and TTLs are centralized as named constants — never hardcode `Duration::from_secs(N)`.

**Domain layer** (`domain/constants.rs` — importable from all layers):

| Constant | Value | Purpose |
|----------|-------|---------|
| `PROVIDER_REQUEST_TIMEOUT` | 300s | Inference request to Ollama/Gemini |
| `OLLAMA_METADATA_TIMEOUT` | 10s | Ollama `/api/show`, `/api/tags`, `/api/ps` |
| `OLLAMA_HEALTH_CHECK_TIMEOUT` | 5s | Ollama `/api/version` in analyzer |
| `LLM_ANALYSIS_TIMEOUT` | 30s | Single-model LLM analysis |
| `LLM_BATCH_ANALYSIS_TIMEOUT` | 60s | Batch model LLM analysis |
| `NODE_EXPORTER_TIMEOUT` | 5s | Node-exporter metrics fetch |
| `CANCEL_TIMEOUT` | 5s | Job cancellation in CancelGuard |
| `OLLAMA_MODEL_CACHE_TTL` | 10s | Provider-for-model lookup cache |
| `MODEL_SELECTION_CACHE_TTL` | 30s | Provider model-selection enabled list cache |
| `HEALTH_CHECK_INTERVAL_SECS` | 30s | Health checker loop interval |
| `STATS_TICK_INTERVAL` | 1s | FlowStats broadcast cadence |

**Health checker** (`health_checker.rs` — health check specific):

| Constant | Value | Purpose |
|----------|-------|---------|
| `OLLAMA_HEALTH_TIMEOUT` | 5s | Ollama `/api/version` health check |
| `GEMINI_HEALTH_TIMEOUT` | 10s | Gemini API key validation |
| `NODE_EXPORTER_METRICS_TIMEOUT` | 5s | node-exporter metrics scrape |

## Provider Liveness — Push Model (Heartbeat)

Scale target: 10,000+ providers, tens of thousands req/s.
Do NOT poll providers directly from veronex.
Use the push model: veronex-agent sets a TTL heartbeat; veronex reads via MGET.

| Component | Responsibility |
|-----------|---------------|
| `veronex-agent/src/heartbeat.rs` | `set_online(pool, provider_id, ttl_secs)` after each successful Ollama scrape |
| `domain::constants::provider_heartbeat_key(id)` | Canonical key: `veronex:provider:hb:{uuid}` (pk-aware shim: `valkey_keys::provider_heartbeat`) |
| `health_checker.rs` | MGET all known heartbeat keys → one round-trip; missing key = offline |
| `domain::constants::PROVIDERS_ONLINE_COUNTER_KEY` | `INCR`/`DECR` atomically on status transitions → O(1) dashboard reads |

## Scale Guards — 10K+ Provider Patterns

| Pattern | Location | Detail |
|---------|----------|--------|
| `MAX_SCORING_CANDIDATES = 50` | `dispatcher.rs` | Bounds scoring loop: O(10K) → O(50) |
| `MAX_CONCURRENT_METRICS = 64` | `health_checker.rs` | Semaphore limits concurrent node-exporter polls |
| `MAX_CONCURRENT_PROBES = 64` | `health_checker.rs` | Semaphore limits HTTP health probes (no-Valkey fallback) |
| `pg_class.reltuples` | `dashboard_queries.rs` | O(1) total_jobs estimate instead of COUNT(*) |
| `join_all` parallelism | `dispatcher.rs`, `placement_planner.rs` | Parallel Valkey/DB calls instead of sequential loops |
| `concurrent_http_probes()` | `health_checker.rs` | Bounded parallel HTTP for MGET fallback |
| No-Valkey DB cache | `background.rs` | DB query every 10s (not 1s) when Valkey absent |

## Orphan Sweeper — Agent-Side Crash Recovery

Detects crashed API instances and fails their orphaned jobs. Runs in `veronex-agent`, not in the API server. API servers manage their own INCR/DECR during normal operation; the agent only intervenes when an API server is confirmed dead.

### Separation of Concerns

| Component | Responsibility |
|-----------|---------------|
| API server (`reaper.rs`) | Heartbeat refresh (SET EX 30s, every 10s) + SADD to `INSTANCES_SET` + re-enqueue orphaned jobs (second chance) |
| Agent (`orphan_sweeper.rs`) | Monitor heartbeats, detect death, fail orphaned jobs in DB, DECR counters, SREM from instance set |

### Instance Registry

| Key | Type | Purpose |
|-----|------|---------|
| `veronex:instances` | SET | All API instance IDs (SADD on heartbeat + startup) |
| `veronex:heartbeat:{id}` | STRING EX 30s | Instance liveness (refreshed every 10s) |
| `veronex:suspect:{id}` | STRING EX 180s | Grace period marker (2-min confirmation) |
| `veronex:reaped:{id}` | STRING NX EX 86400s | Prevents duplicate cleanup (24h) |
| `veronex:job:owner:{uuid}` | STRING EX 300s | Maps running job to owning instance |

### 2-Minute Suspect Grace Period

```
Heartbeat missing → SET suspect EX 180 → wait
TTL drops to ≤ 60  → 2+ minutes elapsed → confirmed dead
SET reaped NX      → claim cleanup (single execution)
```

Network blips (< 2 min) do not trigger cleanup. The suspect marker auto-expires after 3 min if the instance recovers.

### Shard Distribution (10K Scale)

| Sweep | Interval | Scope |
|-------|----------|-------|
| Shard sweep | 30s | `hash(instance_id) % replicas == ordinal` — each agent handles its shard |
| Leader sweep | 60s | NX lock — one agent fails jobs from deleted/inactive providers |

### Cleanup Actions

1. Find jobs owned by dead instance (Valkey `processing` list + `job:owner` keys)
2. UPDATE DB: `status = 'failed'`, `failure_reason = 'server_crash'`
3. LREM from processing list, DEL owner key
4. DECR `JOBS_RUNNING_COUNTER` / `JOBS_PENDING_COUNTER`
5. Belt-and-suspenders: DB query for `instance_id` match (catches jobs not in Valkey list)
6. SREM from `INSTANCES_SET`, DEL suspect marker

### Restart Behavior

All agents down then restart: `tokio::time::interval` fires immediately on first tick, triggering an immediate scan and cleanup of any dead instances found.

