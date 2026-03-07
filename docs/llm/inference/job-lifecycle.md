# Jobs — Core Lifecycle & Queue

> SSOT | **Last Updated**: 2026-03-04

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change job status flow | `domain/enums.rs` → `JobStatus` + all `match` arms in `use_cases/inference.rs` | |
| Add new DB column to inference_jobs | `migrations/` + `domain/entities/mod.rs` + `persistence/job_repository.rs` `save()` | |
| Change queue keys or poll interval | `application/use_cases/inference.rs` → `QUEUE_KEY_API_PAID` / `QUEUE_KEY_API` / `QUEUE_KEY_TEST` constants + `queue_dispatcher_loop()` | |
| Change how tokens are counted | `application/use_cases/inference.rs` → `run_job()` token processing block (streaming loop) | |
| Add field to job list/detail response | See `docs/llm/inference/job-api.md` | |
| Export training data | See `docs/llm/inference/session-grouping.md` | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/veronex/src/domain/enums.rs` | `JobStatus`, `ProviderType`, `JobSource` |
| `crates/veronex/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
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

## Tiered-Queue Architecture

Every inference route goes through the Valkey queue — **no direct-to-provider path exists**.
Three queues in strict priority order (Lua priority pop tries paid → standard → test):

```
Priority  Queue key                    Who gets it
────────  ───────────────────────────  ────────────────────────────────
  HIGH    veronex:queue:jobs:paid      API key tier = "paid"
  MED     veronex:queue:jobs           API key tier = "free" / standard
  LOW     veronex:queue:jobs:test      Test Run (Bearer JWT, no API key)

queue_dispatcher_loop:
  Lua LMOVE [paid, jobs, test] → processing list  — paid tried first
```

- API routes (source=Api) enqueue to `:paid` or `:jobs` based on `key.tier`.
- Test Run routes (source=Test) always enqueue to `:test`.

Constants in `application/use_cases/inference.rs`:
```rust
const QUEUE_KEY_API_PAID: &str = "veronex:queue:jobs:paid";   // tier="paid"
const QUEUE_KEY_API:      &str = "veronex:queue:jobs";         // tier="free"/standard
const QUEUE_KEY_TEST:     &str = "veronex:queue:jobs:test";    // source=Test
```

- `submit()` selects queue by `key_tier` (for Api source) or `source=Test`.
- `recover_pending_jobs()` re-enqueues to the correct queue on startup.
- On no-provider-available: job is LPUSH-ed back to its original queue (preserving priority).

## Job Lifecycle

```
Client → inference route → submit(prompt, model, provider_type, ...) → Pending → RPUSH → queue

queue_dispatcher_loop (Lua priority pop paid/jobs/test → processing):
  → thermal + slot check → run_job() → stream_tokens()
  → Completed: latency_ms, ttft_ms, tokens, result_text, tool_calls_json saved
  → ObservabilityPort → veronex-analytics → OTel → Redpanda → ClickHouse
```

## Entity

Entity: `domain/entities/mod.rs` — `InferenceJob`. Key fields:

| Field | Type | Notes |
|-------|------|-------|
| `id` | `Uuid` | UUIDv7 PK |
| `model_name` | `String` | |
| `provider_type` | `ProviderType` | Ollama / Gemini |
| `status` | `JobStatus` | Pending / Running / Completed / Failed / Cancelled |
| `source` | `JobSource` | Api / Test (immutable) |
| `prompt` | `String` | display prompt (last user message, short) |
| `messages` | `Option<Value>` | full LLM input context (→ `messages_json` JSONB in DB, 100-500 KB for agentic sessions). Note: `messages_json` in the DB may be NULL for new jobs. S3 is the authoritative message store; the DB column is used as a fallback for older jobs. |
| `tools` | `Option<Value>` | in-memory only during dispatch, not persisted |
| `api_key_id` | `Option<Uuid>` | FK → api_keys (ON DELETE SET NULL) |
| `provider_id` | `Option<Uuid>` | FK → llm_providers, set at dispatch time |
| `conversation_id` | `Option<String>` | X-Conversation-ID header; see `session-grouping.md` |
| `tool_calls_json` | `Option<Value>` | model-returned tool calls JSONB |
| `latency_ms` | `Option<i32>` | `started_at` → `completed_at` (excludes queue wait) |
| `ttft_ms` | `Option<i32>` | Time To First Token |
| `queue_time_ms` | `Option<i32>` | `created_at` → `started_at` (queue wait) |
| `cancelled_at` | `Option<DateTime>` | set by cancel(); NULL for non-cancelled jobs |

> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (computed in API, not stored)

---

## JobRepository Patterns

```rust
// infrastructure/outbound/persistence/job_repository.rs (PostgresJobRepository)
save()        // UPSERT ON CONFLICT(id) DO UPDATE (status, result_text, error, timestamps, metrics)
              // source + messages_json: COALESCE — immutable once set
get_status()  // in-memory map first, DB fallback
stream()      // token buffer + tokio::Notify (no polling, no broadcast channel)
```

### In-Memory Job Store (`InferenceUseCaseImpl`)

The live token buffer and job status are held in `Arc<DashMap<Uuid, JobEntry>>` (not Postgres):

```rust
// application/use_cases/inference.rs
struct JobEntry {
    tokens: Vec<InferenceToken>,   // Vec::with_capacity(256) — avoids repeated realloc
    notify: Arc<Notify>,           // wakes stream() consumers on new token
    cancel_notify: Arc<Notify>,    // wakes run_job() cancel branch
    status: JobStatus,
    done: bool,
    tpm_reservation_minute: Option<i64>, // minute bucket for TPM adjustment
}
```

**DashMap rule**: `Ref`/`RefMut` guards must be **dropped before any `.await`** — clone what you need, drop the guard, then await. Holding a guard across `.await` deadlocks the shard.

`PostgresJobRepository.save()` persists final state to DB only on completion/failure; intermediate tokens live only in the DashMap until the stream closes.

**Deferred cleanup**: `run_job` spawns a 60-second delayed `jobs.remove(&uuid)` on every exit path (completed, failed, cancelled). This prevents indefinite memory growth while keeping tokens replayable for late-connecting SSE clients.

---

## Cancellation

### API

```
DELETE /v1/dashboard/jobs/{id}   ← primary (JWT-protected, dashboard use)
DELETE /v1/inference/{job_id}    ← legacy alias (also wired)
    → 200 OK  (idempotent — no-op if already terminal: Completed or Failed)
```

Auth: JWT Bearer (`Authorization: Bearer <token>`) for dashboard endpoint.
API key (`X-API-Key`) is **not** accepted for cancel — dashboard-only operation.

### Cancel Flow

`cancel()` is a **no-op** for terminal states (Completed, Failed, Cancelled). For active jobs:
1. In-memory: `entry.status = Cancelled`, `entry.done = true`, both `notify`s fired
2. `run_job` select! `biased` cancel branch fires — drops stream (broken-pipe stops Ollama)
3. DB: `UPDATE inference_jobs SET status = 'cancelled', cancelled_at = $2 WHERE id = $1 AND status NOT IN ('completed', 'failed')`

### CancelOnDrop — Client Disconnect

Submit-and-stream endpoints wrap SSE/NDJSON in `CancelOnDrop<S>` (`cancel_guard.rs`). On client disconnect, `CancelGuard::drop()` spawns `use_case.cancel(job_id)`. Read-only replay endpoints (`stream_inference`, `stream_job_openai`) are NOT wrapped — multiple clients may share one job.

---

## Related Docs

- Dashboard API & response structs: `docs/llm/inference/job-api.md`
- Session grouping & training data: `docs/llm/inference/session-grouping.md`
- Token cost / pricing: `docs/llm/inference/model-pricing.md`
- Token observability: `docs/llm/inference/job-analytics.md`
- Web UI: `docs/llm/frontend/pages/jobs.md`
