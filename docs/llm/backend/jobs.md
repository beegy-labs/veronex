# Jobs — Lifecycle, Queue & API

> SSOT | **Last Updated**: 2026-03-02 (rev: 3-tier queue paid/api/test; queue_time_ms + cancelled_at fields; GET /v1/dashboard/queue/depth endpoint)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to job list/detail response | `dashboard_handlers.rs` → `JobSummary` / `JobDetail` struct + SQL SELECT | |
| Filter jobs by new criteria | `dashboard_handlers.rs` → `list_jobs()` SQL WHERE clause | |
| Change job status flow | `domain/enums.rs` → `JobStatus` + all `match` arms in `use_cases/inference.rs` | |
| Add new DB column to inference_jobs | `migrations/` + `domain/entities/mod.rs` + `persistence/job_repository.rs` `save()` | |
| Change queue keys or BLPOP timeout | `application/use_cases/inference.rs` → `QUEUE_KEY_API_PAID` / `QUEUE_KEY_API` / `QUEUE_KEY_TEST` constants + `queue_dispatcher_loop()` | |
| Change how tokens are counted | `application/use_cases/inference.rs` → `run_job()` token processing block (streaming loop) | |
| Add new dashboard stat | `dashboard_handlers.rs` → `dashboard_stats()` SQL query | |
| Add SSE replay for a job | `handlers.rs` → `stream_job_openai()` — already implemented for reconnect use-case | |
| Add new field to real-time job event | `domain/value_objects.rs` → `JobStatusEvent` + all `event_tx.send(...)` call sites in `inference.rs` | |
| Export training data | Query `inference_jobs` — join on `conversation_id`; use `messages_json` (input) + `tool_calls_json` + `result_text` (output) | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/inferq/src/domain/enums.rs` | `JobStatus`, `BackendType`, `JobSource` |
| `crates/inferq/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
| `crates/inferq/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `DynamicBackendRouter` (dispatch/routing only) |
| `crates/inferq/src/infrastructure/inbound/http/dashboard_handlers.rs` | Dashboard job list / detail handlers + `job_events_sse` |
| `crates/inferq/src/infrastructure/inbound/http/handlers.rs` | Native inference handlers + `stream_job_openai` |
| `crates/inferq/src/domain/value_objects.rs` | `JobStatusEvent` — real-time event struct |

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

## Tiered-Queue Architecture

Every inference route goes through the Valkey queue — **no direct-to-backend path exists**.
Three queues in strict priority order (BLPOP polls left-to-right):

```
Priority  Queue key                    Who gets it
────────  ───────────────────────────  ────────────────────────────────────────
  HIGH    veronex:queue:jobs:paid      API key tier = "paid"
  MED     veronex:queue:jobs           API key tier = "free" / standard
  LOW     veronex:queue:jobs:test      Test Run (Bearer JWT, no API key)

── API routes (source=Api) ─────────────────────────────────────────────────────
POST /v1/chat/completions           ──▶ :paid  or  :jobs  (by key.tier)
POST /v1/inference                  ──▶ :paid  or  :jobs
POST /api/generate                  ──▶ :paid  or  :jobs
POST /api/chat                      ──▶ :paid  or  :jobs
POST /v1beta/models/*:*Content      ──▶ :paid  or  :jobs

── Test Run routes (source=Test) ───────────────────────────────────────────────
POST /v1/test/completions           ──▶ veronex:queue:jobs:test
POST /v1/test/api/chat              ──▶ veronex:queue:jobs:test
POST /v1/test/api/generate          ──▶ veronex:queue:jobs:test
POST /v1/test/v1beta/models/*       ──▶ veronex:queue:jobs:test

queue_dispatcher_loop:
  BLPOP [veronex:queue:jobs:paid, veronex:queue:jobs, veronex:queue:jobs:test] 5.0
         ↑ paid polled first, test polled last (BLPOP key-order guarantee)
```

Constants in `application/use_cases/inference.rs`:
```rust
const QUEUE_KEY_API_PAID: &str = "veronex:queue:jobs:paid";   // tier="paid"
const QUEUE_KEY_API:      &str = "veronex:queue:jobs";         // tier="free"/standard
const QUEUE_KEY_TEST:     &str = "veronex:queue:jobs:test";    // source=Test
```

- `submit()` selects queue by `key_tier` (for Api source) or `source=Test`.
- `recover_pending_jobs()` re-enqueues to the correct queue on startup.
- On no-backend-available: job is LPUSH-ed back to its original queue (preserving priority).

---

## Job Lifecycle

```
Client → any inference route
  → InferenceUseCaseImpl::submit(prompt, model, backend_type, api_key_id?, account_id?,
                                  source, api_format, messages?, tools?,
                                  request_path?, conversation_id?)
  → InferenceJob created (status=Pending)
  → Valkey RPUSH → queue
  → SSE/NDJSON stream (format per handler)

queue_dispatcher_loop (BLPOP API queue first):
  → thermal + slot check
  → run_job(): OllamaAdapter.stream_tokens()
      messages.is_some() → POST /api/chat  (multi-turn, sends full context)
      messages.is_none() → POST /api/generate  (single prompt)
  → Completed: status, latency_ms, ttft_ms, tokens, result_text, tool_calls_json saved
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
    pub prompt: String,                    // display prompt (short, last user message)
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub api_key_id: Option<Uuid>,          // FK → api_keys (ON DELETE SET NULL)
    pub account_id: Option<Uuid>,          // FK → accounts (Test Run jobs; migration 000037)
    pub backend_id: Option<Uuid>,          // FK → llm_backends (set at dispatch; migration 000039)
    pub api_format: ApiFormat,             // route discriminator (migration 000041)
    pub request_path: Option<String>,      // e.g. "/v1/chat/completions" (migration 000042)
    pub conversation_id: Option<String>,   // X-Conversation-ID header — agent session grouping (migration 000043)
    pub tool_calls_json: Option<serde_json::Value>, // model-returned tool calls JSONB (migration 000043)
    pub messages: Option<serde_json::Value>, // FULL input context — persisted as messages_json JSONB (migration 000045)
    pub tools: Option<serde_json::Value>,  // tool definitions — in-memory only during dispatch (not persisted)
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>, // pure inference start (excludes queue wait)
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>, // set by cancel(); None for non-cancelled jobs
    pub latency_ms: Option<i32>,           // started_at → completed_at
    pub ttft_ms: Option<i32>,              // Time To First Token
    pub queue_time_ms: Option<i32>,        // created_at → started_at (queue wait duration)
    pub completion_tokens: Option<i32>,
    pub prompt_tokens: Option<i32>,        // migration 000029
    pub cached_tokens: Option<i32>,        // migration 000030
}
```

> **`messages`** (→ `messages_json` in DB): the **complete LLM input context** — system prompt, prior conversation turns (user/assistant/tool), current user message, and any file contents injected by the coding agent. Can reach 100–500 KB for agentic sessions. Used as ground-truth training input.
> **`tools`**: forwarded in-memory to the backend during dispatch; never written to DB.
> **`backend_id`**: set by `queue_dispatcher_loop` at dispatch time — `NULL` at submit time.
> **`conversation_id`**: set from the `X-Conversation-ID` header; groups all turns of one agent session.

> `latency_ms` = pure inference time (`started_at` → `completed_at`, excludes queue wait)
> `queue_time_ms` = queue wait time (`created_at` → `started_at`); set by dispatcher at job start
> `cancelled_at` = timestamp of cancellation request; `NULL` for non-cancelled jobs
> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (computed in API, not stored)

## DB Schema

```sql
CREATE TABLE inference_jobs (
    id                UUID         PRIMARY KEY,
    model_name        VARCHAR(255) NOT NULL,
    backend           VARCHAR(50)  NOT NULL,
    status            VARCHAR(20)  NOT NULL DEFAULT 'pending',
    source            VARCHAR(8)   NOT NULL DEFAULT 'api',   -- migration 000031
    prompt            TEXT         NOT NULL DEFAULT '',       -- display prompt (short)
    result_text       TEXT,
    error             TEXT,
    api_key_id        UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    account_id        UUID REFERENCES accounts(id),          -- migration 000037 (Test Run jobs)
    backend_id        UUID REFERENCES llm_backends(id),      -- migration 000039 (set at dispatch)
    api_format        TEXT NOT NULL DEFAULT 'openai_compat', -- migration 000041
    request_path      TEXT,                                  -- migration 000042 ("/v1/chat/completions" etc.)
    conversation_id   TEXT,                                  -- migration 000043 (X-Conversation-ID)
    tool_calls_json   JSONB,                                 -- migration 000043 (model tool calls)
    messages_json     JSONB,                                 -- migration 000045 (FULL input context)
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    cancelled_at      TIMESTAMPTZ,                              -- set by cancel()
    queue_time_ms     INTEGER,                                  -- created_at → started_at
    latency_ms        INTEGER,
    ttft_ms           INTEGER,
    completion_tokens INTEGER,
    prompt_tokens     INTEGER,    -- migration 000029
    cached_tokens     INTEGER     -- migration 000030
);
-- migrations: 000002 CREATE, 000004 result_text, 000014 api_key_id,
--             000015 latency_ms, 000020 ttft_ms+completion_tokens,
--             000029 prompt_tokens, 000030 cached_tokens, 000031 source,
--             000037 account_id, 000039 backend_id, 000041 api_format,
--             000042 request_path, 000043 conversation_id+tool_calls_json,
--             000044 (lab_settings — separate table), 000045 messages_json,
--             000046 queue_time_ms + cancelled_at
CREATE INDEX idx_inference_jobs_source         ON inference_jobs(source);
CREATE INDEX idx_inference_jobs_conversation_id ON inference_jobs(conversation_id)
    WHERE conversation_id IS NOT NULL;
CREATE INDEX idx_inference_jobs_tool_calls      ON inference_jobs USING GIN (tool_calls_json)
    WHERE tool_calls_json IS NOT NULL;
CREATE INDEX idx_inference_jobs_backend_capacity
    ON inference_jobs(backend_id, model_name, created_at DESC)
    WHERE status = 'Completed';
```

---

## Training Data Structure

For fine-tuning or DPO training, each completed job provides:

```sql
-- Export one training example per completed agentic turn
SELECT
    conversation_id,           -- group turns by session
    id,
    created_at,
    model_name,
    api_format,
    prompt_tokens,
    completion_tokens,
    -- INPUT: full context sent to model
    messages_json,             -- system + file contents + conversation history
    -- OUTPUT: model response
    result_text,               -- text output ("..." for tool-call-only turns)
    tool_calls_json            -- tool invocations (NULL for text-only turns)
FROM inference_jobs
WHERE status = 'Completed'
  AND messages_json IS NOT NULL
ORDER BY conversation_id, created_at;
```

**Agentic session structure** (qwen3-coder / Gemini CLI):
- One `conversation_id` = one coding session
- N turns per session, each = one `inference_jobs` row
- Each turn: `messages_json` grows as tool responses are appended
- `prompt_tokens` shows the actual context size (e.g. 24k tokens with file contents)
- Tool-call turns: `result_text = "..."`, `tool_calls_json = [{function: {name, arguments}}]`
- Text turns: `result_text = <actual answer>`, `tool_calls_json = NULL`

**Conversation threading via `X-Conversation-ID` header**:

Clients (Cline, Claude Code, Gemini CLI) must send this header to group turns:
```
X-Conversation-ID: <session-uuid>
```
Without it, `conversation_id IS NULL` and turns cannot be grouped into sessions.

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

### Queue Depth

```
GET /v1/dashboard/queue/depth
    Authorization: Bearer <JWT>
    → { "api_paid": N, "api": N, "test": N, "total": N }
```

Returns current Valkey queue lengths via `LLEN` on all three queue keys. Returns zeros when Valkey is unavailable (fail-open). Used by the Network Flow panel to display a "N waiting" badge on the Queue node. Frontend polls every 3 s (`queueDepthQuery`).

### Real-Time Job Status Stream (Network Flow)

```
GET /v1/dashboard/jobs/stream
    Authorization: Bearer <JWT>
    → SSE stream (persistent connection)
    → event: job_status
    → data: {"id":"<uuid>","status":"pending|running|completed|failed|cancelled","model_name":"...","backend":"...","latency_ms":null|N}
```

Fires one SSE event per job status transition. Backed by a `tokio::sync::broadcast::channel(256)` in `InferenceUseCaseImpl`:
- `submit()` → fires `pending` event
- `run_job()` → fires `running` after job starts; fires `completed`/`failed`/`cancelled` after `job_repo.save()`
- Slow consumers lag-skip (miss events) rather than blocking producers

Client: `web/hooks/use-inference-stream.ts` — `fetch()`-based SSE reader with JWT Bearer auth; exponential backoff reconnect (2s → 30s max).

`JobStatusEvent` (in `domain/value_objects.rs`):
```rust
pub struct JobStatusEvent {
    pub id: String,
    pub status: String,       // "pending" | "running" | "completed" | "failed" | "cancelled"
    pub model_name: String,
    pub backend: String,      // backend name (e.g. "local-ollama")
    pub latency_ms: Option<i32>,
}
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
    pub source: String,               // "api" | "test"
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

When the job IS still active (`pending` or `running`):
1. In-memory: `entry.status = Cancelled`, `entry.done = true`, both `notify`s fired
2. `run_job` select! `biased` cancel branch fires immediately — drops the stream
3. Dropping the stream closes the TCP connection to Ollama (broken-pipe → Ollama stops)
4. DB: `cancel_job(job_id, Utc::now())` → `UPDATE inference_jobs SET status = 'cancelled', cancelled_at = $2 WHERE id = $1 AND status NOT IN ('completed', 'failed')`

---

## Web UI

→ See `docs/llm/frontend/web-jobs.md`

## Token Observability

→ See `docs/llm/backend/jobs-analytics.md`
