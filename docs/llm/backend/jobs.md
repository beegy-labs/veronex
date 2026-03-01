# Jobs — Lifecycle, Queue & API

> SSOT | **Last Updated**: 2026-03-02 (rev: run_job moved to inference.rs; in-memory job store uses DashMap)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to job list/detail response | `dashboard_handlers.rs` → `JobSummary` / `JobDetail` struct + SQL SELECT | |
| Filter jobs by new criteria | `dashboard_handlers.rs` → `list_jobs()` SQL WHERE clause | |
| Change job status flow | `domain/enums.rs` → `JobStatus` + all `match` arms in `use_cases/inference.rs` | |
| Add new DB column to inference_jobs | `migrations/` + `domain/entities/mod.rs` + `persistence/job_repository.rs` `save()` | |
| Change queue keys or BLPOP timeout | `application/use_cases/inference.rs` → `QUEUE_KEY_API` / `QUEUE_KEY_TEST` constants + `queue_dispatcher_loop()` | |
| Change how tokens are counted | `application/use_cases/inference.rs` → `run_job()` token processing block (streaming loop) | |
| Add new dashboard stat | `dashboard_handlers.rs` → `dashboard_stats()` SQL query | |
| Add SSE replay for a job | `handlers.rs` → `stream_job_openai()` — already implemented for reconnect use-case | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/inferq/src/domain/enums.rs` | `JobStatus`, `BackendType`, `JobSource` |
| `crates/inferq/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
| `crates/inferq/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `DynamicBackendRouter` (dispatch/routing only) |
| `crates/inferq/src/infrastructure/inbound/http/dashboard_handlers.rs` | Dashboard job list / detail handlers |
| `crates/inferq/src/infrastructure/inbound/http/handlers.rs` | Native inference handlers + `stream_job_openai` |

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

- Stored in DB (`api_format` column, migration 000041).
- Enables per-format analytics and usage tracking.

---

## Dual-Queue Architecture

Every inference route goes through the Valkey queue (no direct-to-backend path):

```
── API routes (source=Api) ────────────────────────────────────────────────────────
POST /v1/chat/completions           ──▶ veronex:queue:jobs
POST /v1/inference                  ──▶ veronex:queue:jobs
POST /api/generate                  ──▶ veronex:queue:jobs
POST /api/chat                      ──▶ veronex:queue:jobs
POST /v1beta/models/*:*Content      ──▶ veronex:queue:jobs

── Test Run routes (source=Test) ──────────────────────────────────────────────────
POST /v1/test/completions           ──▶ veronex:queue:jobs:test
POST /v1/test/api/chat              ──▶ veronex:queue:jobs:test
POST /v1/test/api/generate          ──▶ veronex:queue:jobs:test
POST /v1/test/v1beta/models/*       ──▶ veronex:queue:jobs:test

queue_dispatcher_loop:
  BLPOP [veronex:queue:jobs, veronex:queue:jobs:test] 5.0
         ↑ API queue polled first (Redis BLPOP key-order guarantee)
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
Client → any inference route
  → InferenceUseCaseImpl::submit(prompt, model, backend_type, api_key_id?, account_id?,
                                  source, api_format, messages?)
  → InferenceJob created (status=Pending)
  → Valkey RPUSH → queue
  → SSE/NDJSON stream (format per handler)

queue_dispatcher_loop (BLPOP API queue first):
  → thermal + slot check
  → run_job(): OllamaAdapter.stream_tokens()
      messages.is_some() → POST /api/chat  (multi-turn)
      messages.is_none() → POST /api/generate  (single prompt)
  → Completed: status, latency_ms, ttft_ms, tokens, result_text saved
  → ObservabilityPort → veronex-analytics → OTel → Redpanda → ClickHouse
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
    pub source: JobSource,                 // Api | Test  (migration 000031)
    pub prompt: String,
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub api_key_id: Option<Uuid>,          // FK → api_keys (ON DELETE SET NULL)
    pub account_id: Option<Uuid>,          // FK → accounts (Test Run jobs; migration 000037)
    pub backend_id: Option<Uuid>,          // FK → llm_backends (set at dispatch, not submit; migration 000039)
    pub api_format: ApiFormat,             // route discriminator (migration 000041)
    pub messages: Option<serde_json::Value>, // multi-turn context — in-memory DashMap only, NOT persisted to DB
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

> **`messages`**: held in the in-memory `DashMap<Uuid, JobEntry>` only — never written to `inference_jobs` table.
> **`backend_id`**: set by `queue_dispatcher_loop` when the job is dispatched to a backend — `NULL` at submit time.

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
    account_id        UUID REFERENCES accounts(id),          -- migration 000037 (Test Run jobs)
    backend_id        UUID REFERENCES llm_backends(id),      -- migration 000039 (set at dispatch)
    api_format        TEXT NOT NULL DEFAULT 'openai_compat', -- migration 000041
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
--             000029 prompt_tokens, 000030 cached_tokens, 000031 source,
--             000037 account_id, 000039 backend_id, 000041 api_format
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
}
```

**DashMap rule**: `Ref`/`RefMut` guards must be **dropped before any `.await`**.
Clone what you need from the guard, then drop it:

```rust
// ✅ Correct — drop RefMut before notify call
let notify = {
    let mut entry = self.jobs.get_mut(&id)?;
    entry.tokens.push(token);
    entry.notify.clone()
};                          // RefMut dropped here
notify.notify_one();        // .await would be safe here

// ❌ Wrong — holding RefMut across .await → deadlock risk
let mut entry = self.jobs.get_mut(&id)?;
entry.tokens.push(token);
entry.notify.notify_one();
some_future.await;          // RefMut still alive — blocks shard
```

`PostgresJobRepository.save()` persists final state to DB only on completion/failure;
intermediate tokens live only in the DashMap until the stream closes.

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

### Backend Guard (`InferenceUseCaseImpl::cancel()`)

`cancel()` is a **no-op** when the job is already in a terminal state:

```rust
// application/use_cases/inference.rs
if entry.status == JobStatus::Completed || entry.status == JobStatus::Failed {
    return Ok(()); // don't override — DB untouched
}
```

This prevents a race where the SSE consumer (e.g. browser tab close / page
navigate) calls `DELETE` after the job has already finished, which would flip
the status from `completed` → `cancelled`.

When the job IS still active (`pending` or `running`):
1. In-memory: `entry.status = Cancelled`, `entry.done = true`, both `notify`s fired
2. `run_job` select! `biased` cancel branch fires immediately — drops the stream
3. Dropping the stream closes the TCP connection to Ollama (broken-pipe → Ollama stops)
4. DB: `UPDATE inference_jobs SET status = 'cancelled'`

**Pending job cancel** (job in queue, not yet dispatched):
- Job dequeued by `DynamicBackendRouter` but status already `Cancelled` → `run_job` cancel branch fires before inference starts

### Frontend Contract (`api-test-panel.tsx`)

- `jobIdRef.current` is cleared to `null` when `[DONE]` is received (natural completion).
- The unmount cleanup only calls `cancelJob()` when `jobIdRef.current` is non-null.
- So: closing a tab **after** the stream completes never triggers a cancel request.

### Network Flow Visualization

`cancelled` is treated as a terminal state identical to `completed`/`failed`:
- `pending|running → cancelled` transition → emits `response` FlowEvent
- Cancelled response bee color: `var(--theme-status-cancelled)` — slate/gray, dimmed (`cc` opacity)
- Cancel button in job detail modal: shown for `pending` and `running` jobs only
  (file: `web/components/job-table.tsx`)

---

## Web UI

→ See `docs/llm/frontend/web-jobs.md`

## Token Observability

→ See `docs/llm/backend/jobs-analytics.md`
