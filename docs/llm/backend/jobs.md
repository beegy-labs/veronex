# Jobs — Lifecycle, Queue & API

> SSOT | **Last Updated**: 2026-03-03 (rev3: session grouping — messages_hash + messages_prefix_hash + daily background loop)

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
| `crates/veronex/src/domain/entities/mod.rs` | `InferenceJob` entity |
| `crates/veronex/src/domain/enums.rs` | `JobStatus`, `ProviderType`, `JobSource` |
| `crates/veronex/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
| `crates/veronex/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `DynamicProviderRouter` (dispatch/routing only) |
| `crates/veronex/src/infrastructure/inbound/http/dashboard_handlers.rs` | Dashboard job list / detail handlers + `job_events_sse` |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | Native inference handlers + `stream_job_openai` |
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
- On no-provider-available: job is LPUSH-ed back to its original queue (preserving priority).

---

## Job Lifecycle

```
Client → any inference route
  → InferenceUseCaseImpl::submit(prompt, model, provider_type, api_key_id?, account_id?,
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
    pub provider_type: ProviderType,           // Ollama | Gemini
    pub status: JobStatus,                     // Pending | Running | Completed | Failed | Cancelled
    pub source: JobSource,                     // Api | Test
    pub prompt: String,                        // display prompt (short, last user message)
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub api_key_id: Option<Uuid>,              // FK → api_keys (ON DELETE SET NULL)
    pub account_id: Option<Uuid>,              // FK → accounts (Test Run jobs)
    pub provider_id: Option<Uuid>,             // FK → llm_providers (set at dispatch)
    pub api_format: ApiFormat,                 // route discriminator
    pub request_path: Option<String>,          // e.g. "/v1/chat/completions"
    pub conversation_id: Option<String>,       // X-Conversation-ID header — agent session grouping
    pub tool_calls_json: Option<serde_json::Value>, // model-returned tool calls JSONB
    pub messages: Option<serde_json::Value>,   // FULL input context — persisted as messages_json JSONB
    pub tools: Option<serde_json::Value>,      // tool definitions — in-memory only during dispatch (not persisted)
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,     // pure inference start (excludes queue wait)
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,   // set by cancel(); None for non-cancelled jobs
    pub latency_ms: Option<i32>,               // started_at → completed_at
    pub ttft_ms: Option<i32>,                  // Time To First Token
    pub queue_time_ms: Option<i32>,            // created_at → started_at (queue wait duration)
    pub completion_tokens: Option<i32>,
    pub prompt_tokens: Option<i32>,
    pub cached_tokens: Option<i32>,
}
```

> **`messages`** (→ `messages_json` in DB): the **complete LLM input context** — system prompt, prior conversation turns (user/assistant/tool), current user message, and any file contents injected by the coding agent. Can reach 100–500 KB for agentic sessions. Used as ground-truth training input.
> **`tools`**: forwarded in-memory to the provider during dispatch; never written to DB.
> **`provider_id`**: set by `queue_dispatcher_loop` at dispatch time — `NULL` at submit time.
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
    provider_type     VARCHAR(50)  NOT NULL,
    status            VARCHAR(20)  NOT NULL DEFAULT 'pending',
    source            VARCHAR(8)   NOT NULL DEFAULT 'api',
    prompt            TEXT         NOT NULL DEFAULT '',       -- display prompt (short)
    result_text       TEXT,
    error             TEXT,
    api_key_id        UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    account_id        UUID REFERENCES accounts(id),          -- Test Run jobs
    provider_id       UUID REFERENCES llm_providers(id),     -- set at dispatch
    api_format        TEXT NOT NULL DEFAULT 'openai_compat',
    request_path      TEXT,                                  -- "/v1/chat/completions" etc.
    conversation_id      TEXT,                                  -- X-Conversation-ID or batch-assigned
    tool_calls_json      JSONB,                                 -- model tool calls
    messages_json        JSONB,                                 -- FULL input context
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    started_at           TIMESTAMPTZ,
    completed_at         TIMESTAMPTZ,
    cancelled_at         TIMESTAMPTZ,                           -- set by cancel()
    queue_time_ms        INTEGER,                               -- created_at → started_at
    latency_ms           INTEGER,
    ttft_ms              INTEGER,
    completion_tokens    INTEGER,
    prompt_tokens        INTEGER,
    cached_tokens        INTEGER,
    messages_hash        TEXT,       -- Blake2b-256 of full messages array
    messages_prefix_hash TEXT        -- Blake2b-256 of messages[0..-1], "" for first turn
);
-- single init migration: 0000000001_init.sql
CREATE INDEX idx_inference_jobs_source         ON inference_jobs(source);
CREATE INDEX idx_inference_jobs_conversation_id ON inference_jobs(conversation_id)
    WHERE conversation_id IS NOT NULL;
CREATE INDEX idx_inference_jobs_tool_calls      ON inference_jobs USING GIN (tool_calls_json)
    WHERE tool_calls_json IS NOT NULL;
CREATE INDEX idx_inference_jobs_provider_capacity
    ON inference_jobs(provider_id, model_name, created_at DESC)
    WHERE status = 'Completed';
CREATE INDEX idx_inference_jobs_messages_hash
    ON inference_jobs(api_key_id, messages_hash)
    WHERE messages_hash IS NOT NULL;
CREATE INDEX idx_inference_jobs_session_ungrouped
    ON inference_jobs(api_key_id, messages_prefix_hash, created_at)
    WHERE conversation_id IS NULL AND messages_prefix_hash IS NOT NULL AND messages_prefix_hash != '';
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

**Conversation threading — two mechanisms:**

| 방법 | 동작 | 적용 대상 |
|------|------|----------|
| `X-Conversation-ID` 헤더 | 클라이언트가 UUID를 직접 전송 → 즉시 반영 | Cline, Claude Code, Gemini CLI 등 헤더 지원 클라이언트 |
| 서버 배치 자동 추론 | `run_session_grouping_loop` (1일 1회) — 없음 → 자동 할당 | Qwen Code, Cursor 등 헤더 미지원 클라이언트 |

**배치 자동 추론 알고리즘** (`infrastructure/outbound/session_grouping.rs`):
- Job 저장 시 Blake2b-256으로 `messages_hash` + `messages_prefix_hash` 계산
- `messages_prefix_hash` = hash(messages[0..-1]) — 마지막 user 메시지 제외
- 배치: job B의 `messages_prefix_hash` == job A의 `messages_hash` → 같은 `conversation_id`
- 첫 턴(`messages_prefix_hash = ""`): 새 `conversation_id` (UUIDv7) 생성
- `SESSION_GROUPING_INTERVAL_SECS` 환경변수로 주기 조정 (기본: 86400s = 24h)
- Race condition 없음 — 완료된 job 이력 기반, LLM 호출 없음
- **날짜 커트오프**: `created_at < DATE_TRUNC('day', NOW())` — 오늘 진행 중인 대화는 건드리지 않음
- **동시 실행 방지**: `session_grouping_lock: Arc<Semaphore(1)>` (AppState) — 배치/수동 트리거 모두 공유
- **일주일+ 이전 데이터도 묶음** — 시간 상한선 없음; LIMIT 10000으로 매일 점진 처리

**수동 즉시 실행** (`POST /v1/dashboard/session-grouping/trigger`, JWT Bearer):
```json
// Request (optional before_date — 생략 시 오늘 자정 기준)
{ "before_date": "2026-03-01" }

// 202 Accepted — 백그라운드 실행, 즉시 반환
{ "message": "session grouping triggered" }

// 409 Conflict — 이미 실행 중
{ "message": "session grouping already in progress" }
```

**공개 함수**: `group_sessions_before(pg_pool, cutoff: Option<NaiveDate>)` — 핸들러와 배치 루프 모두 호출

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
    → data: {"id":"<uuid>","status":"pending|running|completed|failed|cancelled","model_name":"...","provider_type":"...","latency_ms":null|N}
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
    pub status: String,        // "pending" | "running" | "completed" | "failed" | "cancelled"
    pub model_name: String,
    pub provider_type: String, // provider type (e.g. "ollama" | "gemini")
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
    pub provider_type: String,
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
    pub account_name: Option<String>, // LEFT JOIN accounts (test run jobs)
    pub request_path: Option<String>, // e.g. "/v1/chat/completions"
    pub has_tool_calls: bool,         // true when tool_calls_json IS NOT NULL
    pub estimated_cost_usd: Option<f64>, // NULL = no pricing data; 0.0 = Ollama; >0 = Gemini
}

pub struct JobDetail {
    // All JobSummary fields +
    pub started_at: Option<String>,
    pub prompt: String,               // last user message (display; NOT full context)
    pub result_text: Option<String>,  // None when model responded with tool calls
    pub error: Option<String>,
    pub tool_calls_json: Option<Vec<ToolCall>>, // populated when model used function calls
    pub message_count: Option<i64>,   // JSONB array length of messages_json (conversation turns)
    pub estimated_cost_usd: Option<f64>,
}
```

> **`result_text` vs `tool_calls_json`**: When a model responds with function calls (agentic loop turn), `result_text = NULL` and `tool_calls_json` is populated. The UI renders a Tool Calls section in these cases instead of showing "(no result stored)".

> **`estimated_cost_usd`**: Computed via a LATERAL JOIN on `model_pricing`. Ollama always returns `0.0` (self-hosted = no cost). Gemini returns the actual cost per 1M tokens × token counts. `NULL` means no pricing row found (unknown provider or no seed data).

---

## Token Cost Measurement

Token costs are computed at query time via a LATERAL JOIN against the `model_pricing` table — no cost is stored in `inference_jobs` itself.

### `model_pricing` Table

```sql
CREATE TABLE model_pricing (
    provider      TEXT    NOT NULL,
    model_name    TEXT    NOT NULL,   -- exact name or '*' for wildcard fallback
    input_per_1m  FLOAT8  NOT NULL DEFAULT 0,  -- USD per 1M prompt tokens
    output_per_1m FLOAT8  NOT NULL DEFAULT 0,  -- USD per 1M completion tokens
    currency      TEXT    NOT NULL DEFAULT 'USD',
    PRIMARY KEY (provider, model_name)
);
```

- Ollama: **no rows** — `CASE WHEN provider_type = 'ollama' THEN 0.0` always applies.
- Gemini: exact model rows + `'*'` wildcard fallback (seeded with 2026-03 Google AI pricing).

### LATERAL JOIN Pattern

```sql
LEFT JOIN LATERAL (
    SELECT input_per_1m, output_per_1m
    FROM model_pricing
    WHERE provider = j.provider_type
      AND (model_name = j.model_name OR model_name = '*')
    ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
    LIMIT 1
) pricing ON true
```

Cost expression used in `JobDetail`, `JobSummary`, `UsageBreakdownResponse`:
```sql
CASE
    WHEN j.provider_type = 'ollama' THEN 0.0
    WHEN pricing.input_per_1m IS NOT NULL
         AND j.prompt_tokens IS NOT NULL
         AND j.completion_tokens IS NOT NULL THEN
        (j.prompt_tokens::float8 / 1000000.0 * pricing.input_per_1m) +
        (j.completion_tokens::float8 / 1000000.0 * pricing.output_per_1m)
    ELSE NULL
END AS estimated_cost_usd
```

### Usage Breakdown Cost Aggregation

`GET /v1/usage/breakdown` → `UsageBreakdownResponse`:
- `by_provider[*].estimated_cost_usd` — total cost for that provider in the window
- `by_key[*].estimated_cost_usd` — total cost per API key
- `by_model[*].estimated_cost_usd` — total cost per model+provider combination
- `total_cost_usd` — sum of all provider costs (for the breakdown card header KPI)

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

### Provider Guard (`InferenceUseCaseImpl::cancel()`)

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
