# Distributed: Ops & Registry

> SSOT | **Last Updated**: 2026-05-02 | Classification: Operational
> Cross-instance pub/sub, TPM accounting, crash recovery, Valkey key registry, and wiring.

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
| VRAM sync | on change | Atomic Lua eval (ACQUIRE/RELEASE) updates `veronex:vram_reserved:{pid}` HASH + `veronex:vram_leases:{pid}` ZSET |
| Queue reaper | 60s | Lua CAS on processing list: atomically verify dead owner + re-enqueue |

## SSE Connection Limiter + Timeout

**Files**: `infrastructure/inbound/http/handlers.rs`, `constants.rs`, `state.rs`

### Connection Limiter
Global `Arc<AtomicU32>` counter in `AppState` tracks active SSE connections. Each SSE handler calls `try_acquire_sse()` before creating a stream:
- Returns HTTP 429 if `active >= SSE_MAX_CONNECTIONS` (100).
- `SseDropGuard` RAII struct auto-decrements the counter when the SSE stream is dropped (client disconnect or job completion).
- Applied to all API-key-authenticated SSE endpoints: `/v1/inference/{id}/stream`, `/v1/jobs/{id}/stream`, `/v1/chat/completions` (streaming), `/v1beta/models/{path}` (Gemini streaming).

### Hard Timeout
`with_sse_timeout()` wraps every SSE stream with a `SSE_TIMEOUT` (1700s ≈ 28 min) deadline. Strictly less than the upstream Cilium HTTPRoute `timeouts.request=1800s` so the client always sees a clean `event: error data: stream timeout` rather than an opaque gateway 504. Full timeout invariant chain (SSE / `INFERENCE_ROUTER_TIMEOUT` / `MCP_ROUND_TOTAL_TIMEOUT` / Cilium 1800s) → `inference/mcp.md § timeouts`.
- Uses `async_stream::stream!` with `tokio::select!` + `sleep_until(deadline)`.
- On timeout: sends `event: error` with `data: stream timeout`, then closes the stream.
- Prevents zombie SSE connections that neither complete nor disconnect (e.g., crashed client behind a proxy that keeps TCP alive).
- Applied to all SSE endpoints alongside the connection limiter.

## Valkey Key Registry — two layers

| Layer | Module | Caller |
|-------|--------|--------|
| Canonical (unprefixed) | `domain/constants.rs` — `*_key()` builder fns + `QUEUE_*` consts | application code (only domain import allowed) |
| pk-aware shim | `infrastructure/outbound/valkey_keys.rs` — `pk(&domain::*_key())` | infrastructure that bypasses `ValkeyPort` and talks to fred directly |

`ValkeyAdapter` applies `pk()` automatically inside every key-taking method
(`kv_set`, `kv_get`, `kv_del`, `incr_by`, `queue_*`, `list_*`, `zset_claim`'s
`processing_key` arg, …). So application passes canonical keys; the
deployment-time `VALKEY_KEY_PREFIX` is enforced at the port boundary only.

**Key prefix**: call `valkey_keys::init_prefix(prefix)` once at startup (before any Valkey ops) to prepend a deployment-level namespace to every key. Default `""` = no prefix. Example: `init_prefix("prod:")` → `"prod:veronex:queue:zset"`.

> Full Valkey key catalog (30+ patterns) → `infra/deploy.md § Valkey Key Patterns`. This page focuses on the cross-instance distributed-coordination subset; `deploy.md` is the SSOT for the comprehensive list.

## Wiring (`main.rs`)

1. `init_tracing()` — must come first so subsequent log lines are captured (reads `OTEL_EXPORTER_OTLP_ENDPOINT` directly).
2. Parse `AppConfig::from_env()` — every other env var lives here (single source).
3. Call `valkey_keys::init_prefix(&config.valkey_key_prefix)` — must run before any Valkey op.
4. Connect Postgres pool (`database::connect(&url, config.pg_pool_max)`).
5. Connect Valkey pool when `VALKEY_URL` set; build `ValkeyAdapter::new(pool)` and call `adapter.warmup().await?` to `SCRIPT LOAD` all Lua scripts (priority pop / ZSET enqueue / claim / cancel) — subsequent calls use `EVALSHA`.
6. Resolve `instance_id` (config field, default UUIDv7) — passed to `InferenceUseCaseImpl`.
7. Create `DistributedVramPool` when Valkey available, else `VramPool`.
8. Wire repositories + AppState (composition root).
9. Start job event subscriber (dedicated `SubscriberClient`).
10. Start cancel subscriber (dedicated `SubscriberClient` with psubscribe).
11. Start reaper loop (heartbeat + slot reap + queue reap).
12. Initialize `sse_connections: Arc<AtomicU32>` in `AppState`.
13. Start job sweeper (orphaned DashMap entry cleanup every 5 min).
14. Connect MCP servers concurrently (`session_mgr.connect` × N via `join_all`) and run initial tool discovery in parallel.

## Key Prefix (`VALKEY_KEY_PREFIX`)

Optional deployment-level namespace for all Valkey keys. Allows multiple deployments to share a single Valkey instance without key collision.

| Env var | Default | Example |
|---------|---------|---------|
| `VALKEY_KEY_PREFIX` | `""` (no prefix) | `"prod:"` → keys become `"prod:veronex:queue:zset"` |

Set in `config.rs` → `valkey_key_prefix`. Called as `valkey_keys::init_prefix(&config.valkey_key_prefix)` at startup. Zero-cost no-op when empty.

## Dev Mode

When `VALKEY_URL` is not set:
- `VramPool` (local only) is used.
- Queue is disabled (direct spawn).
- Pub/sub and reaper are not started.
- Single-instance behavior is unchanged.
- SSE connection limiter still active (local counter only).

## Constants Architecture

Domain and application-layer constants live in `domain/constants.rs` (SSOT). Grouped by concern:

### Job lifecycle / queue

| Constant | Value | Purpose |
|----------|-------|---------|
| `TPM_ESTIMATED_TOKENS` | `500` | Tokens reserved per request at admission |
| `JOB_CLEANUP_DELAY` | `60s` | Deferred DashMap entry removal |
| `OWNERSHIP_LOST_CLEANUP_DELAY` | `5s` | Fast cleanup when ownership lost |
| `QUEUE_POLL_INTERVAL` | `500ms` | Empty-queue sleep interval |
| `NO_PROVIDER_BACKOFF` | `2s` | No-provider re-queue backoff |
| `QUEUE_ERROR_BACKOFF` | `1s` | Queue pop error backoff |
| `JOB_OWNER_TTL_SECS` | `300` | Valkey owner key TTL |
| `OWNER_REFRESH_INTERVAL` | `60s` | Owner key refresh interval |
| `INITIAL_TOKEN_CAPACITY` | `256` | Per-job token Vec initial capacity |
| `LEASE_ATTEMPTS_TTL_SECS` | `86400` | Per-job lease-attempts counter TTL |
| `INSTANCE_HEARTBEAT_TTL_SECS` | `30` | Per-instance heartbeat TTL (3× refresh) |

### MCP / lifecycle phase timeouts (coupled across `bridge.rs` + `lifecycle.rs`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `MCP_LIFECYCLE_LOAD_TIMEOUT` | `600s` | Phase-1 cold-load (200K-context worst case) |
| `MCP_TOKEN_FIRST_TIMEOUT` | `300s` | Phase-2 first-token after `phase_boundary` |
| `MCP_STREAM_IDLE_TIMEOUT` | `45s` | Per-token stream-idle ceiling |
| `MCP_ROUND_TOTAL_TIMEOUT` | `1500s` | Hard cap per MCP round (under Cilium 1800s) |
| `MCP_TOOL_REFRESH_INTERVAL` | `25s` | Background tool-discovery refresh |

### Cache TTLs

| Constant | Value | Purpose |
|----------|-------|---------|
| `API_KEY_CACHE_TTL` | `60s` | Per-hash API key cache (hot path) |
| `LAB_SETTINGS_CACHE_TTL` | `30s` | Lab settings single-row cache |
| `MODEL_SELECTION_CACHE_TTL` | `30s` | Provider→enabled-models cache |
| `OLLAMA_MODEL_CACHE_TTL` | `10s` | Per-provider model list cache |
| `PROVIDER_REGISTRY_CACHE_TTL` | `5s` | In-memory registry snapshot |
| `CONV_CACHE_TTL_SECS` | `300` | Conversation record + S3-detail cache |
| `MCP_KEY_CACHE_TTL_SECS` | `60` | Per-API-key MCP caches (acl/cap/topk) |
| `MCP_TOOLS_SUMMARY_TTL_SECS` | `3600` | MCP tools_summary cache |
| `OLLAMA_MODEL_CTX_TTL_SECS` | `600` | Ollama per-(provider, model) context window |
| `MODELS_CACHE_TTL_SECS` | `3600` | Per-provider model list (Valkey) |
| `SERVICE_HEALTH_TTL_SECS` | `60` | Per-instance service-health HASH |

### Auth / rate limiting

| Constant | Value | Purpose |
|----------|-------|---------|
| `PASSWORD_RESET_TTL_SECS` | `86400` | Password-reset token (24h) |
| `LOGIN_ATTEMPTS_WINDOW_SECS` | `300` | Per-IP login-attempts sliding window (also `Retry-After`) |
| `RATE_LIMIT_RETRY_AFTER_SECS` | `60` | Default 429 fallback Retry-After |
| `KEY_TIER_PAID` | `"paid"` | API key billing tier value |
| `GEMINI_TIER_FREE` | `"free"` | Gemini free-tier routing value |
| `API_KEY_PREFIX` | `"vnx_"` | Prefix for generated API key plaintexts |

### Placement / scaleout

| Constant | Value | Purpose |
|----------|-------|---------|
| `PRELOAD_LOCK_TTL_SECS` | `180` | Preload NX-lock TTL (cold-load window) |
| `SCALEOUT_DECISION_TTL_SECS` | `30` | Scale-out decision dedup TTL |

HTTP-specific constants remain in `infrastructure/inbound/http/constants.rs` (SSE_*, INFERENCE_ROUTER_TIMEOUT, JWT_ROUTER_TIMEOUT, body limits) with re-exports from `domain::constants` for convenience.

### Canonical Valkey-key constructors / string consts (SSOT)

`domain/constants.rs` also owns every canonical (unprefixed) Valkey key. Application code imports from here; `ValkeyAdapter` applies the deployment prefix automatically. Examples: `job_owner_key`, `conversation_record_key`, `heartbeat_key`, `ratelimit_tpm_key`, `preload_lock_key`, `scaleout_decision_key`, `demand_key`, `mcp_tool_key`, …; string consts: `JOBS_PENDING_COUNTER_KEY`, `JOBS_RUNNING_COUNTER_KEY`, `PROVIDERS_ONLINE_COUNTER_KEY`, `INSTANCES_SET_KEY`, `AGENT_INSTANCES_SET_KEY`, `PUBSUB_JOB_EVENTS_KEY`, `PUBSUB_CANCEL_PATTERN_KEY`, `PUBSUB_CANCEL_PREFIX_KEY`, `VRAM_LEASES_SCAN_PATTERN_KEY`.

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
| `broadcast_event()` | `inference/use_case.rs` | 3× event_tx.send + pub/sub publish |
| `schedule_cleanup()` | `inference/use_case.rs` | 4× tokio::spawn + sleep + remove pattern |

## Files Changed

| File | Change |
|------|--------|
| `valkey_keys.rs` | 8 key functions; token key changed from `pubsub:tokens` → `stream:tokens` |
| `concurrency_port.rs` | `VramPoolPort` trait: `try_reserve()`, `VramPermit` RAII, `provider_active_requests()` (O(1) via `total_active_count`) |
| `capacity/vram_pool.rs` | `VramPool` per-provider VRAM tracking + O(1) provider-total active count (`total_active_count`) |
| `capacity/analyzer.rs` | Unified sync: health + model + VRAM probe; LLM correction increase-only |
| `capacity/thermal.rs` | Per-provider `ThermalThresholds` (GPU/CPU presets), `pre_hard_total` in `ThrottleState`, Soft hysteresis `active_count==0`, RampUp→Normal `Σmc≥pre_hard_total` |
| **NEW** `capacity/distributed_vram_pool.rs` | `DistributedVramPool` with Valkey pub/sub sync |
| `application/use_cases/inference.rs` | Lua priority pop, VRAM permit enforcement (direct+queue), model selection filter, stickiness bonus, thermal per-provider gate |
| **NEW** `domain/constants.rs` | Application-layer constants (moved from infrastructure) |
| **NEW** `infrastructure/outbound/pubsub/relay.rs` | Publish + subscribe helpers; **token XADD (Streams)** + cleanup |
| **NEW** `infrastructure/outbound/pubsub/reaper.rs` | Heartbeat + slot/queue reaper; **Lua CAS for atomic re-enqueue** |
| `infrastructure/outbound/hw_metrics.rs` | `gpu_vendor` field added to `HwMetrics` |
| `infrastructure/outbound/health_checker.rs` | Auto-set thermal thresholds from `gpu_vendor` per cycle |
| `main.rs` | model_manager=None, model_selection_repo wiring, distributed setup |
| `Cargo.toml` | Fred features: `subscriber-client`, `i-pubsub`, **`i-streams`** |
