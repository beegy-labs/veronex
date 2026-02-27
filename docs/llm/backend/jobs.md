# Jobs — Lifecycle, Queue & API

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to job list/detail response | `infrastructure/inbound/http/handlers.rs` → `JobSummary` / `JobDetail` struct + SQL SELECT |
| Filter jobs by new criteria | `handlers.rs` → `list_jobs()` SQL WHERE clause |
| Change job status flow | `domain/enums/` → `JobStatus` + all `match` arms in `use_cases/inference.rs` |
| Add new DB column to inference_jobs | `migrations/` + `domain/entities/inference_job.rs` + `persistence/backend_registry.rs` `save()` |
| Change queue key or BLPOP timeout | `application/use_cases/inference.rs` → `queue_dispatcher_loop()` |
| Change how tokens are counted | `infrastructure/outbound/backend_router.rs` → `run_job()` token processing block |
| Add new dashboard stat | `handlers.rs` → `dashboard_stats()` SQL query |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/inference_job.rs` | `InferenceJob` entity |
| `crates/inferq/src/domain/enums/` | `JobStatus`, `BackendType` |
| `crates/inferq/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl` (submit, stream, dispatch loop) |
| `crates/inferq/src/infrastructure/outbound/persistence/backend_registry.rs` | `PostgresJobRepository` (UPSERT) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `DynamicBackendRouter` + `run_job()` |
| `crates/inferq/src/infrastructure/inbound/http/handlers.rs` | Dashboard + native inference handlers |

---

## Job Lifecycle

```
Client → POST /v1/chat/completions (or /v1/inference)
  → InferenceUseCaseImpl::submit()
  → InferenceJob created (status=Pending, api_key_id set)
  → Valkey RPUSH veronex:queue:jobs
  → SSE stream (OpenAI) or { job_id } (native)

queue_dispatcher_loop (BLPOP veronex:queue:jobs):
  → DynamicBackendRouter::dispatch()
  → run_job() (started_at recorded)
  → OllamaAdapter | GeminiAdapter → SSE tokens
  → Completed: status=Completed, completed_at, latency_ms, ttft_ms, completion_tokens, result_text saved
  → ObservabilityPort::record_inference() → ClickHouse inference_logs
```

---

## Entity

```rust
// domain/entities/inference_job.rs
pub struct InferenceJob {
    pub id: Uuid,
    pub model_name: String,
    pub backend: BackendType,              // Ollama | Gemini
    pub status: JobStatus,                 // Pending | Running | Completed | Failed | Cancelled
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
    prompt            TEXT         NOT NULL DEFAULT '',
    result_text       TEXT,
    error             TEXT,
    api_key_id        UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    latency_ms        INTEGER,
    ttft_ms           INTEGER,            -- migration 000020
    completion_tokens INTEGER            -- migration 000020
);
-- migrations: 000002 CREATE, 000004 result_text, 000014 api_key_id,
--             000015 latency_ms, 000020 ttft_ms+completion_tokens
```

---

## JobRepository Patterns

```rust
// infrastructure/outbound/persistence/backend_registry.rs (PostgresJobRepository)
save()        // UPSERT ON CONFLICT(id) DO UPDATE (status, result_text, error, timestamps, metrics)
get_status()  // in-memory map first, DB fallback
stream()      // token buffer + tokio::Notify (no polling, no broadcast channel)
```

---

## API Endpoints

### Dashboard

```
GET /v1/dashboard/stats
    → { total_keys, active_keys, total_jobs, jobs_last_24h, jobs_by_status }

GET /v1/dashboard/jobs?limit=&offset=&status=&q=
    q      → prompt ILIKE '%{q}%'
    status → all | pending | running | completed | failed | cancelled
    → { total: i64, jobs: Vec<JobSummary> }

GET /v1/dashboard/jobs/{id}
    → JobDetail
```

### JobSummary / JobDetail

```rust
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub backend: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub completion_tokens: Option<i64>,
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
