# Multi-Instance Architecture

> **SSOT** for distributed coordination across Veronex replicas.

## Problem

With `replicas > 1` in Kubernetes (HPA/KEDA), five subsystems use instance-local state:

| Subsystem | Local state | Multi-instance issue |
|-----------|------------|---------------------|
| VRAM pool | DashMap + AtomicU32 | Each instance tracks independently → N× intended allocation |
| Job queue | BLPOP | At-most-once → crash after pop = lost job |
| Token streaming | DashMap | SSE client on instance B can't see tokens from instance A |
| Job status events | broadcast::Sender | Dashboard on instance B misses events from instance A |
| Cancellation | cancel_notify | Cancel on instance B can't interrupt job on instance A |

## Solution

All fixes use **Valkey** (already a dependency) as the distributed coordination layer. The hexagonal architecture is preserved: new adapters implement existing ports.

## Instance Identity

Each process generates `instance_id = Uuid::new_v4()` at startup (in `main.rs`). Stored as `Arc<str>`, passed to adapters that need it.

## Distributed VRAM Pool

**File**: `infrastructure/outbound/capacity/distributed_vram_pool.rs`

| Component | Purpose |
|-----------|---------|
| `DistributedVramPool` | Implements `VramPoolPort` using local `VramPool` + Valkey pub/sub for state sync |

**Strategy**: Local `VramPool` provides fast sync reserve (per-instance). Valkey pub/sub broadcasts VRAM state changes across instances for visibility. Each instance maintains its own `ProviderVramState` (total_mb, reserved_kv_mb, loaded_models, model_profiles).

**RAII**: `VramPermit` releases KV cache on drop (weight stays loaded). This is synchronized across instances via async Valkey publish.

**Lease management**: Each VRAM reservation creates a ZSET lease entry with 120s TTL. The lease member format is `instance_id:lease_id:kv_mb`, embedding the KV allocation size for crash recovery.

**Lease lifecycle** (Lua scripts):
- `LUA_VRAM_ACQUIRE`: `HINCRBY reserved +kv` + `ZADD lease` (member includes kv_mb)
- `LUA_VRAM_RELEASE`: `HINCRBY reserved -kv` + `ZREM lease`
- `LUA_VRAM_REAP`: `ZRANGEBYSCORE expired` → for each: `ZREM` + `HINCRBY reserved -kv` (extracted from member)

The reaper deducts reserved HASH on lease expiry, preventing zombie reservations after instance crashes.

## Reliable Queue

**File**: `application/use_cases/inference.rs` — `queue_dispatcher_loop()`

| Before | After |
|--------|-------|
| `BLPOP` from 3 queues (at-most-once) | Lua priority pop into `veronex:queue:processing` list |
| Crash after pop = lost job | Processing list + `veronex:job:owner:{job_id}` tracks ownership |
| No crash recovery | Reaper re-enqueues orphaned jobs |

**Model filter**: Two-stage filtering for Ollama jobs in the queue dispatcher:
1. `providers_for_model()` — filters providers that have the requested model installed (OllamaModelRepository).
2. `list_enabled()` — filters providers where the model is disabled in selection config (ProviderModelSelectionRepository). Same check as `provider_router`'s direct path.

**Model stickiness**: Providers with the requested model already loaded in VRAM get a +100GB bonus in the availability sort, strongly favoring consecutive requests on the same provider over model switching.

**Lua `LUA_PRIORITY_POP`**: Tries LMOVE from paid → standard → test into processing list. Returns nil if all empty. Dispatcher sleeps 500ms on nil.

**ACK**: On job completion/failure, `LREM processing 1 {uuid}` + `DEL job:owner:{job_id}`.

**Re-queue on no-provider**: `LREM processing` + `LPUSH` back to source queue.

### Double-Execution Prevention

Three-layer defense against the same job running on two instances simultaneously:

1. **Lua CAS in reaper** (`LUA_REAP_OWNED_JOB`): atomically checks that `job:owner` still matches the expected dead instance AND that its heartbeat key is absent, before re-enqueueing. No TOCTOU race.
2. **Periodic owner refresh** in `run_job()`: every 60s refreshes `job:owner` TTL via `SET XX EX 300`. Prevents the reaper from seeing a stale owner key during long-running jobs.
3. **Ownership guard before final DB write** in `run_job()`: verifies `GET job:owner:{job_id}` still matches `instance_id` before persisting results. Aborts silently if ownership was transferred.

**Ownerless jobs**: Reaper uses `LUA_REAP_OWNERLESS_JOB` with `SET NX` to claim ownership before re-enqueue, preventing multiple reapers from racing.

## Cross-Instance Pub/Sub

**File**: `infrastructure/outbound/pubsub/relay.rs`

### Job Events
- **Publisher**: After each `event_tx.send()` in `run_job()`, also `PUBLISH veronex:pubsub:job_events`.
- **Subscriber**: Background task subscribes, deserializes events, forwards to local `broadcast::Sender`. Dedup by `instance_id`.

### Cancellation
- **Publisher**: `cancel()` publishes to `veronex:pubsub:cancel:{job_id}` when job is not local.
- **Subscriber**: Pattern-subscribe `veronex:pubsub:cancel:*`, extract `job_id`, fire `cancel_notify` if job is local.
- **cancel_notifiers**: `Arc<DashMap<Uuid, Arc<Notify>>>` shared between `InferenceUseCaseImpl` and the cancel subscriber.

### Token Streaming (Valkey Streams)
- **`publish_token()`** uses **XADD** (Valkey Streams) instead of plain PUB/SUB.
- `MAXLEN ~ 500` caps stream size to prevent unbounded growth.
- **Atomic `LUA_XADD_EXPIRE`**: XADD + `EXPIRE 600s` in a single Lua eval — prevents orphaned stream keys from accumulating after job completion or crash.
- Late-connecting subscribers read from `0-0` to catch up — **no initial token black hole**.
- `cleanup_token_stream()` DELs the stream key after job completes (immediate cleanup). The 600s EXPIRE is a safety net for crash scenarios.
- Not yet wired into `run_job()` token loop (deferred: only needed when cross-instance SSE is active).

## TPM Accounting

Rate limiter reserves 500 tokens per request at admission. After job completion, `record_tpm()` adjusts: `actual_tokens - 500`.

**Error path**: Failed jobs call `record_tpm(pool, key_id, 0, tpm_reservation_minute)` — refunds the full 500-token reservation. Without this, failed requests would permanently consume quota.

## Job Sweeper

**File**: `application/use_cases/inference.rs` — `start_job_sweeper()`

Background task (every 300s) scans DashMap for Pending entries older than 10 minutes and removes them. Covers the multi-instance orphan scenario: instance A submits (DashMap insert), instance B dispatches and runs, instance A's entry is never cleaned by `run_job`.

Also cleans up corresponding `cancel_notifiers` entries to prevent notify handle leaks.

## Crash Recovery + Reaper

**File**: `infrastructure/outbound/pubsub/reaper.rs`

| Task | Interval | Action |
|------|----------|--------|
| Heartbeat | 10s | `SET veronex:heartbeat:{instance_id} 1 EX 30` |
| VRAM sync | on change | Publish VRAM state updates via Valkey pub/sub |
| Queue reaper | 60s | Lua CAS on processing list: atomically verify dead owner + re-enqueue |

## SSE Connection Limiter + Timeout

**Files**: `infrastructure/inbound/http/handlers.rs`, `constants.rs`, `state.rs`

### Connection Limiter
Global `Arc<AtomicU32>` counter in `AppState` tracks active SSE connections. Each SSE handler calls `try_acquire_sse()` before creating a stream:
- Returns HTTP 429 if `active >= SSE_MAX_CONNECTIONS` (100).
- `SseDropGuard` RAII struct auto-decrements the counter when the SSE stream is dropped (client disconnect or job completion).
- Applied to all API-key-authenticated SSE endpoints: `/v1/inference/{id}/stream`, `/v1/jobs/{id}/stream`, `/v1/chat/completions` (streaming), `/v1beta/models/{path}` (Gemini streaming).

### Hard Timeout
`with_sse_timeout()` wraps every SSE stream with a `SSE_TIMEOUT` (600s / 10 min) deadline:
- Uses `async_stream::stream!` with `tokio::select!` + `sleep_until(deadline)`.
- On timeout: sends `event: error` with `data: stream timeout`, then closes the stream.
- Prevents zombie SSE connections that neither complete nor disconnect (e.g., crashed client behind a proxy that keeps TCP alive).
- Applied to all SSE endpoints alongside the connection limiter.

## Valkey Key Registry

All keys defined in `infrastructure/outbound/valkey_keys.rs`:

| Key | Type | TTL | Purpose |
|-----|------|-----|---------|
| `veronex:heartbeat:{iid}` | STRING | 30s | Instance liveness |
| `veronex:vram:{pid}` | HASH | - | Per-instance VRAM reservation state |
| `veronex:queue:processing` | LIST | - | Reliable queue processing set |
| `veronex:job:owner:{job_id}` | STRING | 300s | Which instance owns a running job |
| `veronex:stream:tokens:{job_id}` | STREAM | 600s | Cross-instance token relay (XADD/XREAD) |
| `veronex:pubsub:job_events` | PUB/SUB | - | Cross-instance job status events |
| `veronex:pubsub:cancel:{job_id}` | PUB/SUB | - | Cross-instance cancel signals |

## Wiring (`main.rs`)

1. Generate `instance_id` at startup.
2. Create `DistributedVramPool` when Valkey available, else `VramPool`.
3. Pass `instance_id` to `InferenceUseCaseImpl`.
4. Start job event subscriber (dedicated `SubscriberClient`).
5. Start cancel subscriber (dedicated `SubscriberClient` with psubscribe).
6. Start reaper loop (heartbeat + slot reap + queue reap).
7. Initialize `sse_connections: Arc<AtomicU32>` in `AppState`.
8. Start job sweeper (orphaned DashMap entry cleanup every 5 min).

## Dev Mode

When `VALKEY_URL` is not set:
- `VramPool` (local only) is used.
- Queue is disabled (direct spawn).
- Pub/sub and reaper are not started.
- Single-instance behavior is unchanged.
- SSE connection limiter still active (local counter only).

## Constants Architecture

Domain and application-layer constants live in `domain/constants.rs`:

| Constant | Value | Purpose |
|----------|-------|---------|
| `GEMINI_TIER_FREE` | `"free"` | Gemini free-tier routing value |
| `KEY_TIER_PAID` | `"paid"` | API key billing tier for paid keys |
| `TPM_ESTIMATED_TOKENS` | `500` | Tokens reserved per request at admission |
| `JOB_CLEANUP_DELAY` | `60s` | Deferred DashMap entry removal |
| `OWNERSHIP_LOST_CLEANUP_DELAY` | `5s` | Fast cleanup when ownership lost |
| `QUEUE_POLL_INTERVAL` | `500ms` | Empty-queue sleep interval |
| `NO_PROVIDER_BACKOFF` | `2s` | No-provider re-queue backoff |
| `QUEUE_ERROR_BACKOFF` | `1s` | Queue pop error backoff |
| `JOB_OWNER_TTL_SECS` | `300` | Valkey owner key TTL |
| `OWNER_REFRESH_INTERVAL` | `60s` | Owner key refresh interval |
| `INITIAL_TOKEN_CAPACITY` | `256` | Per-job token Vec initial capacity |

HTTP-specific constants remain in `infrastructure/inbound/http/constants.rs` (SSE_*, MODELS_CACHE_TTL) with re-exports from `domain::constants` for convenience.

## Shared Handler Helpers

Deduplication helpers in `infrastructure/inbound/http/`:

| Helper | File | Replaces |
|--------|------|----------|
| `sse_response()` | `handlers.rs` | 7× SSE assembly pattern across 5 files |
| `parse_uuid()` | `handlers.rs` | 13× `Uuid::parse_str().map_err()` across 4 files |
| `SseStream` type alias | `handlers.rs` | 5× duplicate type alias definitions |
| `CompletionChunk::content/stop/finish/tool_calls` | `openai_sse_types.rs` | Verbose struct construction in 3 files |
| `get_provider()` | `provider_handlers.rs` | 7× fetch-or-404 pattern |
| `get_gpu_server()` | `gpu_server_handlers.rs` | 2× fetch-or-404 pattern |
| `broadcast_event()` | `inference.rs` | 3× event_tx.send + pub/sub publish |
| `schedule_cleanup()` | `inference.rs` | 4× tokio::spawn + sleep + remove pattern |

## Files Changed

| File | Change |
|------|--------|
| `valkey_keys.rs` | 8 key functions; token key changed from `pubsub:tokens` → `stream:tokens` |
| `concurrency_port.rs` | `VramPoolPort` trait: `try_reserve()`, `VramPermit` RAII, `provider_active_requests()` |
| `capacity/vram_pool.rs` | `VramPool` per-provider VRAM tracking + provider-total active count |
| `capacity/analyzer.rs` | Unified sync: health + model + VRAM probe |
| `capacity/thermal.rs` | Per-provider `ThermalThresholds` (GPU/CPU presets), auto-detect from `gpu_vendor` |
| **NEW** `capacity/distributed_vram_pool.rs` | `DistributedVramPool` with Valkey pub/sub sync |
| `application/use_cases/inference.rs` | Lua priority pop, VRAM permit enforcement (direct+queue), model selection filter, stickiness bonus, thermal per-provider gate |
| **NEW** `domain/constants.rs` | Application-layer constants (moved from infrastructure) |
| **NEW** `infrastructure/outbound/pubsub/relay.rs` | Publish + subscribe helpers; **token XADD (Streams)** + cleanup |
| **NEW** `infrastructure/outbound/pubsub/reaper.rs` | Heartbeat + slot/queue reaper; **Lua CAS for atomic re-enqueue** |
| `infrastructure/outbound/hw_metrics.rs` | `gpu_vendor` field added to `HwMetrics` |
| `infrastructure/outbound/health_checker.rs` | Auto-set thermal thresholds from `gpu_vendor` per cycle |
| `main.rs` | model_manager=None, model_selection_repo wiring, distributed setup |
| `veronex-agent/src/main.rs` | `gpu_vendor` from sysfs vendor ID (AMD/NVIDIA detection) |
| `Cargo.toml` | Fred features: `subscriber-client`, `i-pubsub`, **`i-streams`** |
