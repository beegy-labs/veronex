# Jobs — Core Lifecycle & Queue

> SSOT | **Last Updated**: 2026-04-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change job status flow | `domain/enums.rs` → `JobStatus` + all `match` arms in `use_cases/inference/runner.rs` | |
| Add new DB column to inference_jobs | `docker/postgres/init.sql` + `domain/entities/mod.rs` + `persistence/job_repository.rs` `save()` | |
| Change queue keys or scoring | `domain/constants.rs` → `QUEUE_ZSET`, `TIER_BONUS_*`, `LOCALITY_BONUS_MS` + `dispatcher.rs` → `queue_dispatcher_loop()` | |
| Change how tokens are counted | `use_cases/inference/runner.rs` → `run_job()` token processing block (streaming loop) | |
| Add field to job list/detail response | See `docs/llm/inference/job-api.md` | |
| Export training data | See `docs/llm/inference/session-grouping.md` | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/veronex/src/domain/enums.rs` | `JobStatus`, `ProviderType`, `JobSource` |
| `crates/veronex/src/application/use_cases/inference/` | Module: `use_case.rs` (submit, stream), `dispatcher.rs` (queue loop), `runner.rs` (run_job), `helpers.rs` (broadcast, TPM) |
| `crates/veronex/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `DynamicProviderRouter` (dispatch/routing only) |
| `crates/veronex/src/domain/value_objects.rs` | `JobStatusEvent` — real-time event struct |

---

## Job Source (`JobSource`)

Jobs carry a `source` field that records their origin:

| Value | Meaning |
|-------|---------|
| `api` | Submitted by any API key route (`/v1/chat/completions`, `/api/chat`, `/api/generate`, `/v1beta/models/*`, `/v1/inference`) |
| `test` | Submitted from the dashboard Test Run panel (`/v1/test/*` routes, Bearer JWT, no rate limit) |
| `analyzer` | Submitted by the capacity analyzer for VRAM probing and batch analysis (internal LLM inference) |

- The `source` field is **immutable** — set at creation, never updated on UPSERT.
- Default value in DB: `'api'` (backward-compatible with older rows).

---

## API Format (`ApiFormat`)

`api_format` records which API wire format the request arrived via (route-based discriminator):

| Value | Routes |
|-------|--------|
| `OpenaiCompat` | `POST /v1/chat/completions`, `POST /v1/test/completions` |
| `OllamaNative` | `POST /api/generate`, `POST /api/chat`, `POST /v1/test/api/generate`, `POST /v1/test/api/chat` |
| `GeminiNative` | `POST /v1beta/models/*`, `POST /v1/test/v1beta/models/*` |
| `VeronexNative`| `POST /v1/inference` |

- Stored in DB (`api_format` column).
- Enables per-format analytics and usage tracking.

---

## ZSET Priority Queue (Phase 3)

Every inference route attempts Valkey ZSET queuing first. If Valkey is unavailable or returns an error (or when `VALKEY_URL` is not configured), the job falls back to `spawn_job_direct()` — a direct async task without queue ordering or retry. On the direct path, if VRAM is unavailable at dispatch time, the job is silently dropped (warning logged) with no re-enqueue.
Single unified ZSET with tier-based scoring (lower score = higher priority):

```
score = now_ms - tier_bonus

Tier       Bonus (ms)   Effect
────────   ──────────   ─────────────────────────
paid       300,000      Highest priority (lowest score)
standard   100,000      Default API key tier
test       0            Lowest priority (Test Run / dashboard)
```

Enqueue: Lua atomic (ZCARD guard + per-model demand guard + ZADD + INCR demand + HSET×2).
Dispatch: ZRANGE peek top-K → Rust scoring (locality + age × perf_factor) → Lua claim (ZREM queue:zset + ZADD queue:active score=deadline_ms + DECR demand + HDEL side hashes).

### Lease Queue (`queue:active`)

Claimed jobs move from `queue:zset` into `queue:active` (ZSET, score = lease deadline unix_ms). The worker renews the lease every `LEASE_RENEW_INTERVAL_SECS` (30s) via a keepalive task. If the lease expires before renewal, the processing reaper re-enqueues the job (up to `LEASE_MAX_ATTEMPTS` times), then permanently fails it.

| Constant | Value | Notes |
|----------|-------|-------|
| `QUEUE_ACTIVE` | `veronex:queue:active` | ZSET, score = deadline_ms |
| `QUEUE_ACTIVE_ATTEMPTS` | `veronex:queue:active:attempts` | Hash: job_id → attempt count |
| `LEASE_TTL_MS` | 90,000 ms | Lease lifetime; worker must renew before expiry |
| `LEASE_RENEW_INTERVAL_SECS` | 30s | Keepalive renew cadence |
| `PROCESSING_REAPER_SECS` | 30s | Reaper scan interval (registered in `bootstrap/background.rs`) |
| `LEASE_MAX_ATTEMPTS` | 2 | Max re-enqueues before permanent failure (`lease_expired_max_attempts`) |

Constants in `domain/constants.rs`:
```rust
pub const QUEUE_ZSET: &str = "veronex:queue:zset";
pub const QUEUE_ACTIVE: &str = "veronex:queue:active";
pub const QUEUE_ACTIVE_ATTEMPTS: &str = "veronex:queue:active:attempts";
pub const LEASE_TTL_MS: u64 = 90_000;
pub const LEASE_RENEW_INTERVAL_SECS: u64 = 30;
pub const PROCESSING_REAPER_SECS: u64 = 30;
pub const LEASE_MAX_ATTEMPTS: u64 = 2;
pub const TIER_BONUS_PAID: u64 = 300_000;
pub const TIER_BONUS_STANDARD: u64 = 100_000;
pub const TIER_BONUS_TEST: u64 = 0;
pub const LOCALITY_BONUS_MS: f64 = 20_000.0;  // loaded model preference
pub const MAX_QUEUE_SIZE: u64 = 10_000;        // global hard cap → 429
pub const MAX_QUEUE_PER_MODEL: u64 = 2_000;    // per-model cap → 429
```

- `submit()` computes score from `key_tier` / `source` and calls `zset_enqueue()`.
- `recover_pending_jobs()` re-enqueues to ZSET with emergency priority on startup.
- On cancel: Lua atomic ZREM + DECR demand + HDEL side hashes.
- On no-provider (VRAM blocked): job stays in ZSET (not removed), dispatcher retries next loop.

## Job Lifecycle

```
Client → inference route → submit(prompt, model, ...) → Pending → ZADD queue:zset (score=now_ms-tier_bonus)

queue_dispatcher_loop (ZRANGE peek → Rust scoring → Lua ZREM claim → ZADD queue:active score=deadline_ms):
  → keepalive task renews lease every 30s → run_job():
       Phase 1 (MCP_LIFECYCLE_PHASE=on): provider.ensure_ready(model)  ← see flows/model-lifecycle.md
       Phase 2: provider.stream_tokens(&job)
  → Completed: finalize() writes metrics to Postgres + ConversationRecord to S3
  → ObservabilityPort → veronex-analytics → OTel → Redpanda → ClickHouse
```

Phase 1 fails → `failure_reason = "lifecycle_failed"`, no Phase 2 attempt,
running counter decremented, schedule_cleanup fires. Flag default `false`
preserves pre-Tier-C behaviour (implicit auto-load via `stream_tokens`).

## Entity

Entity: `domain/entities/mod.rs` — `InferenceJob`. Key fields:

| Field | Type | Notes |
|-------|------|-------|
| `id` | `Uuid` | UUIDv7 PK |
| `model_name` | `String` | |
| `provider_type` | `ProviderType` | Ollama / Gemini |
| `status` | `JobStatus` | Pending / Running / Completed / Failed / Cancelled |
| `source` | `JobSource` | Api / Test (immutable) |
| `prompt_preview` | `Option<String>` | ≤200 chars of prompt, CJK-safe truncation with `…` — DB only, full prompt in S3 |
| `messages` | `Option<Value>` | in-memory during dispatch; **not persisted to DB** — stored in S3 `ConversationRecord` |
| `tools` | `Option<Value>` | in-memory only during dispatch, not persisted |
| `has_tool_calls` | `bool` | `TRUE` when model emitted tool/function calls — lightweight flag for list view |
| `api_key_id` | `Option<Uuid>` | FK → api_keys (ON DELETE SET NULL) |
| `provider_id` | `Option<Uuid>` | FK → llm_providers (ON DELETE SET NULL), set at dispatch time |
| `conversation_id` | `Option<String>` | X-Conversation-ID header; see `session-grouping.md` |
| `latency_ms` | `Option<i32>` | `started_at` → `completed_at` (excludes queue wait) |
| `ttft_ms` | `Option<i32>` | Time To First Token |
| `queue_time_ms` | `Option<i32>` | `created_at` → `started_at` (queue wait) |
| `cancelled_at` | `Option<DateTime>` | set by cancel(); NULL for non-cancelled jobs |
| `image_keys` | `Option<Vec<String>>` | S3 object keys for attached images (WebP); stored as `TEXT[]` in DB |
| `mcp_loop_id` | `Option<Uuid>` | groups jobs in one MCP agentic loop |
| `failure_reason` | `Option<String>` | machine-readable failure cause: `queue_full`, `no_eligible_provider`, `queue_wait_exceeded`, `provider_error`, `token_budget_exceeded`, `lease_expired_max_attempts`, `lease_expired_reenqueue_failed`, `lifecycle_failed` |
| `account_id` | `Option<Uuid>` | account that submitted via Test Run |

**S3 ConversationRecord** (`conversations/{owner_id}/{YYYY-MM-DD}/{job_id}.json.zst`):

| Field | Type | Notes |
|-------|------|-------|
| `prompt` | `String` | full original prompt |
| `messages` | `Option<Value>` | full LLM input context (100-500 KB for agentic sessions) |
| `tool_calls` | `Option<Value>` | all tool/function calls emitted (MCP + OpenAI function calls) |
| `result` | `Option<String>` | final text output |

Written by `finalize_job()` (happy path) **OR** `persist_partial_conversation()` (cancel / stream-error / lifecycle-failed paths) using zstd-3 compression (~1.2 KB / record). Read on-demand by the admin detail view (one S3 GET per click). `owner_id = account_id ?? api_key_id ?? job_id`.

Per-job idempotency is enforced via `JobEntry::persisted_to_s3: Arc<AtomicBool>` — `compare_exchange(false, true)` ensures exactly-one S3 PUT across racing finalize ↔ cancel paths inside `run_job`'s biased `select!`. MCP-loop jobs (`mcp_loop_id.is_some()`) skip the runner-side persist — bridge owns those via the post-loop write block in `bridge::run_loop` (gate: `!content.is_empty() || !all_mcp_tool_calls.is_empty()`, so partial tool-call state is preserved even if the loop exits early).

> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (computed in API, not stored)

→ `job-lifecycle-impl.md` — JobRepository patterns, in-memory DashMap store, cancellation, related docs
