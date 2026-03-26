# Inference Request Lifecycle

> **Last Updated**: 2026-03-26

---

## End-to-End Flow

```
Client
  │  POST /v1/chat/completions  (Bearer API Key or session JWT)
  ▼
InferCaller middleware
  ├── API Key path  → BLAKE2b hash → DB lookup → RPM/TPM rate limit check
  └── JWT path      → verify HS256 → extract account_id
  │
  ▼
openai_handlers::chat_completions()
  │
  ├─ [MCP intercept?] should_intercept() == true
  │     └─→ mcp_ollama_chat() → bridge.run_loop()     ← see flows/mcp.md
  │
  └─ [normal path]
       │
       ▼
  use_case.submit(SubmitJobRequest)
       │
       ├── validate model name / provider type
       ├── check global_model_disabled
       ├── check key provider access (api_key_provider_access)
       ├── enforce MAX_QUEUE_SIZE / MAX_QUEUE_PER_MODEL
       ├── persist job to DB (status = Queued)
       ├── push JobId to DashMap<Uuid, JobEntry>
       └── return JobId
       │
       ▼
  Queue dispatcher loop (background task, woken by Notify)
       │
       ▼
  dispatcher::queue_dispatcher_loop()
       │
       ├── dequeue next job (priority: paid > standard > free, then age)
       ├── select_provider()                           ← see flows/scheduler.md
       │     ├── thermal gate (Soft/Hard → skip)
       │     ├── circuit breaker (open → skip)
       │     ├── VRAM check (available_vram_mb > needed)
       │     └── score providers → pick highest
       │
       ├── vram_pool.reserve(provider_id, model)       ← acquire KV permit
       │
       └── spawn_job_direct(job_id, provider_id)
             │
             ▼
       runner::run_job()
             │
             ├── build Ollama /api/chat request
             ├── POST to provider URL (streaming)
             ├── emit tokens → broadcast_channel
             │     └── SSE handler consumes stream → Client
             ├── on completion: record prompt/completion tokens
             ├── vram_pool.release(provider_id, model)
             ├── update job status → Completed/Failed
             ├── emit observability event (OTel → ClickHouse)
             └── schedule_cleanup(job_id, TTL=300s)
```

---

## Queue Priority Scoring

```
score = age_ms
      + tier_bonus          (paid=3000, standard=1000, free=0)
      - thermal_penalty     (perf_factor * age_bonus reduction)

Dispatch order: highest score wins
```

---

## Key Constants

| Constant | Value | Location |
|----------|-------|----------|
| `MAX_QUEUE_SIZE` | 512 | `domain/constants.rs` |
| `MAX_QUEUE_PER_MODEL` | 64 | `domain/constants.rs` |
| `TIER_BONUS_PAID` | 3000ms | `domain/constants.rs` |
| `TIER_BONUS_STANDARD` | 1000ms | `domain/constants.rs` |
| Job cleanup TTL | 300s | `use_case/helpers.rs` |

---

## Data Path

```
PostgreSQL
  inference_jobs (status, provider_id, tokens, latency)

ClickHouse (via OTel)
  inference_logs (per-request analytics)
  inference_sessions (grouped conversation analytics)

Valkey
  veronex:ratelimit:rpm:{key_id}   sorted set, TTL=62s
  veronex:ratelimit:tpm:{key_id}:{minute}  counter, TTL=120s
  veronex:heartbeat:{provider_id}  liveness, TTL=180s
```

---

## Files

| File | Purpose |
|------|---------|
| `infrastructure/inbound/http/openai_handlers.rs` | Entry point, MCP dispatch decision |
| `infrastructure/inbound/http/middleware/infer_auth.rs` | `InferCaller` extractor |
| `application/use_cases/inference/use_case.rs` | `submit()`, job lifecycle |
| `application/use_cases/inference/dispatcher.rs` | Queue loop, provider selection |
| `application/use_cases/inference/runner.rs` | Job execution, streaming |
