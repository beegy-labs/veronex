# Jobs — Dashboard API & Response Structs

> SSOT | **Last Updated**: 2026-03-16

## API Endpoints

### Dashboard

```
GET /v1/dashboard/stats
    → { total_keys, active_keys, total_jobs, jobs_last_24h, jobs_by_status }

GET /v1/dashboard/jobs?limit=&offset=&status=&q=&source=&model=&provider=
    q        → prompt ILIKE '%{q}%'
    status   → all | pending | running | completed | failed | cancelled
    source   → api | test | analyzer  (omit for all)
    model    → exact match on model_name (omit for all)
    provider → exact match on provider name via JOIN (omit for all)
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

### `flow_stats` SSE Event

In addition to `job_status` events, the stream emits `flow_stats` every 1 second:

```
event: flow_stats
data: {"incoming": 2, "queued": 5, "running": 3, "completed": 120}
```

`FlowStats` struct: `{ incoming: u32, queued: u32, running: u32, completed: u32 }`. Used by the Network Flow panel to render live flow chart badges (pending/running/req-s).

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

---

## Response Structs

### JobSummary

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
    pub provider_name: Option<String>,   // LEFT JOIN llm_providers (server name)
}
```

### JobDetail

```rust
pub struct JobDetail {
    // All JobSummary fields +
    pub started_at: Option<String>,
    pub prompt: String,               // last user message (display; NOT full context)
    pub result_text: Option<String>,  // None when model responded with tool calls
    pub error: Option<String>,
    pub messages_json: Option<serde_json::Value>, // full conversation context (JSONB from DB)
    pub tool_calls_json: Option<serde_json::Value>, // model-returned tool calls (JSONB)
    pub message_count: Option<i64>,   // JSONB array length of messages_json (conversation turns)
    pub estimated_cost_usd: Option<f64>,
    pub image_keys: Option<Vec<String>>,  // S3 object keys for attached images
    pub image_urls: Option<Vec<String>>,  // presigned/direct URLs resolved from image_keys
}
```

> **`result_text` vs `tool_calls_json`**: When a model responds with function calls (agentic loop turn), `result_text = NULL` and `tool_calls_json` is populated. The UI renders a Tool Calls section in these cases instead of showing "(no result stored)".

> **`estimated_cost_usd`**: Computed via a LATERAL JOIN on `model_pricing`. Ollama always returns `0.0` (self-hosted = no cost). Gemini returns the actual cost per 1M tokens x token counts. `NULL` means no pricing row found (unknown provider or no seed data). See `docs/llm/inference/model-pricing.md`.

---

## Related Docs

- Job lifecycle & entity: `docs/llm/inference/job-lifecycle.md`
- Model pricing computation: `docs/llm/inference/model-pricing.md`
- Frontend jobs UI: `docs/llm/frontend/pages/jobs.md`
- Session grouping: `docs/llm/inference/session-grouping.md`
