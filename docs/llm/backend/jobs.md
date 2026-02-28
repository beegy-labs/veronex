# Jobs — Lifecycle, Queue & API

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to job list/detail response | `dashboard_handlers.rs` → `JobSummary` / `JobDetail` struct + SQL SELECT | |
| Filter jobs by new criteria | `dashboard_handlers.rs` → `list_jobs()` SQL WHERE clause | |
| Change job status flow | `domain/enums.rs` → `JobStatus` + all `match` arms in `use_cases/inference.rs` | |
| Add new DB column to inference_jobs | `migrations/` + `domain/entities/mod.rs` + `persistence/job_repository.rs` `save()` | |
| Change queue keys or BLPOP timeout | `application/use_cases/inference.rs` → `QUEUE_KEY_API` / `QUEUE_KEY_TEST` constants + `queue_dispatcher_loop()` | |
| Change how tokens are counted | `infrastructure/outbound/backend_router.rs` → `run_job()` token processing block | |
| Add new dashboard stat | `dashboard_handlers.rs` → `dashboard_stats()` SQL query | |
| Add SSE replay for a job | `handlers.rs` → `stream_job_openai()` — already implemented for reconnect use-case | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/inferq/src/domain/enums.rs` | `JobStatus`, `BackendType`, `JobSource` |
| `crates/inferq/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
| `crates/inferq/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `DynamicBackendRouter` + `run_job()` |
| `crates/inferq/src/infrastructure/inbound/http/dashboard_handlers.rs` | Dashboard job list / detail handlers |
| `crates/inferq/src/infrastructure/inbound/http/handlers.rs` | Native inference handlers + `stream_job_openai` |

---

## Job Source (`JobSource`)

Jobs carry a `source` field that records their origin:

| Value | Meaning |
|-------|---------|
| `api` | Submitted by an API client via `POST /v1/chat/completions` or `POST /v1/inference` |
| `test` | Submitted from the dashboard test panel (`source: "test"` in request body) |

- The `source` field is **immutable** — set at creation, never updated on UPSERT.
- Default value in DB: `'api'` (backward-compatible with older rows).

---

## Dual-Queue Architecture

```
POST /v1/chat/completions (source=api)  ──▶ veronex:queue:jobs          (API queue)
POST /v1/chat/completions (source=test) ──▶ veronex:queue:jobs:test     (test queue)

queue_dispatcher_loop:
  BLPOP [veronex:queue:jobs, veronex:queue:jobs:test] 5.0
         ↑ API queue is always polled first (Redis BLPOP key-order guarantee)
```

Constants in `application/use_cases/inference.rs`:
```rust
const QUEUE_KEY_API:  &str = "veronex:queue:jobs";
const QUEUE_KEY_TEST: &str = "veronex:queue:jobs:test";
```

- `submit()` chooses the queue based on `source`.
- `recover_pending_jobs()` re-enqueues to the correct queue on startup.
- On no-backend-available: job is LPUSH-ed back to its original queue (preserving priority).

---

## Job Lifecycle

```
Client → POST /v1/chat/completions (or /v1/inference)
  → InferenceUseCaseImpl::submit(source)
  → InferenceJob created (status=Pending, api_key_id set, source set)
  → Valkey RPUSH veronex:queue:jobs  (or :test)
  → SSE stream (OpenAI) or { job_id } (native)

queue_dispatcher_loop (BLPOP API queue first, then test queue):
  → DynamicBackendRouter::dispatch()
  → run_job() (started_at recorded)
  → OllamaAdapter | GeminiAdapter → SSE tokens
  → Completed: status=Completed, completed_at, latency_ms, ttft_ms, completion_tokens, result_text saved
  → ObservabilityPort::record_inference() → Redpanda → ClickHouse inference_logs
```

---

## Entity

```rust
// domain/entities/mod.rs — InferenceJob
pub struct InferenceJob {
    pub id: Uuid,
    pub model_name: String,
    pub backend: BackendType,              // Ollama | Gemini
    pub status: JobStatus,                 // Pending | Running | Completed | Failed | Cancelled
    pub source: JobSource,                 // Api | Test  ← NEW (migration 000031)
    pub prompt: String,
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub api_key_id: Option<Uuid>,          // FK → api_keys (ON DELETE SET NULL)
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>, // pure inference start (excludes queue wait)
    pub completed_at: Option<DateTime<Utc>>,
    pub latency_ms: Option<i32>,           // started_at → completed_at
    pub ttft_ms: Option<i32>,              // Time To First Token
    pub completion_tokens: Option<i32>,
    pub prompt_tokens: Option<i32>,        // migration 000029
    pub cached_tokens: Option<i32>,        // migration 000030
}
```

> `latency_ms` = pure inference time (excludes queue wait)
> `created_at - started_at` = queue wait time
> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (computed in API, not stored)

## DB Schema

```sql
CREATE TABLE inference_jobs (
    id                UUID         PRIMARY KEY,
    model_name        VARCHAR(255) NOT NULL,
    backend           VARCHAR(50)  NOT NULL,
    status            VARCHAR(20)  NOT NULL DEFAULT 'pending',
    source            VARCHAR(8)   NOT NULL DEFAULT 'api',   -- migration 000031
    prompt            TEXT         NOT NULL DEFAULT '',
    result_text       TEXT,
    error             TEXT,
    api_key_id        UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    latency_ms        INTEGER,
    ttft_ms           INTEGER,
    completion_tokens INTEGER,
    prompt_tokens     INTEGER,    -- migration 000029
    cached_tokens     INTEGER     -- migration 000030
);
-- migrations: 000002 CREATE, 000004 result_text, 000014 api_key_id,
--             000015 latency_ms, 000020 ttft_ms+completion_tokens,
--             000029 prompt_tokens, 000030 cached_tokens, 000031 source
CREATE INDEX idx_inference_jobs_source ON inference_jobs(source);
```

---

## JobRepository Patterns

```rust
// infrastructure/outbound/persistence/job_repository.rs (PostgresJobRepository)
save()        // UPSERT ON CONFLICT(id) DO UPDATE (status, result_text, error, timestamps, metrics)
              // source is excluded from ON CONFLICT update — immutable once set
get_status()  // in-memory map first, DB fallback
stream()      // token buffer + tokio::Notify (no polling, no broadcast channel)
```

---

## API Endpoints

### Dashboard

```
GET /v1/dashboard/stats
    → { total_keys, active_keys, total_jobs, jobs_last_24h, jobs_by_status }

GET /v1/dashboard/jobs?limit=&offset=&status=&q=&source=
    q      → prompt ILIKE '%{q}%'
    status → all | pending | running | completed | failed | cancelled
    source → api | test  (omit for all)
    → { total: i64, jobs: Vec<JobSummary> }

GET /v1/dashboard/jobs/{id}
    → JobDetail
```

### SSE Replay (Test Reconnect)

```
GET /v1/jobs/{id}/stream
    Authorization: X-API-Key required
    → OpenAI-format SSE stream (replays completed result or live tokens)
    → data: {"choices":[{"delta":{"content":"..."}}]}
    → data: [DONE]
```

Used by the test panel to reconnect to an in-progress or completed stream after page navigation.

### JobSummary / JobDetail

```rust
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub backend: String,
    pub status: String,
    pub source: String,               // "api" | "test"  ← NEW
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub tps: Option<f64>,             // completion_tokens / (latency_ms - ttft_ms) * 1000
    pub api_key_name: Option<String>, // LEFT JOIN api_keys
}

pub struct JobDetail { /* all JobSummary fields + started_at, prompt, result_text, error */ }
```

---

## Web UI

→ See `docs/llm/frontend/web-jobs.md`

## Token Observability

→ See `docs/llm/backend/jobs-analytics.md`
