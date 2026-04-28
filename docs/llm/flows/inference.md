# Inference Request Lifecycle

> **Last Updated**: 2026-04-28

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
       ├── persist job to DB (status = Pending)
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
             ├── [MCP_LIFECYCLE_PHASE=on]?            ← see flows/model-lifecycle.md
             │     └── provider.ensure_ready(model)   ← Phase 1 — explicit load probe
             │           ├── VramPool warm? → AlreadyLoaded
             │           ├── Coalesced? → LoadCoalesced{waited_ms}
             │           ├── Cold load → LoadCompleted (≤ LIFECYCLE_LOAD_TIMEOUT 600s)
             │           └── Err(LifecycleError) → mark failed (failure_reason=lifecycle_failed)
             │
             ├── provider.stream_tokens(&job)         ← Phase 2 — inference
             │     └── POST /api/chat or /api/generate (streaming)
             ├── emit tokens → broadcast_channel
             │     └── SSE handler consumes stream → Client
             ├── on completion: record prompt/completion tokens
             ├── vram_pool.release(provider_id, model)
             ├── update job status → Completed/Failed
             ├── emit observability event (OTel → ClickHouse)
             └── schedule_cleanup(job_id, TTL=60s)
```

---

## Queue Priority Scoring

```
score = now_ms.saturating_sub(tier_bonus)
  paid:     tier_bonus = 300,000ms (300s)  → lower score → higher priority
  standard: tier_bonus = 100,000ms (100s)
  free:     tier_bonus = 0

Dispatch order: lowest score wins (ZRANGEBYSCORE)
```

---

## Key Constants

| Constant | Value | Location |
|----------|-------|----------|
| `MAX_QUEUE_SIZE` | 10,000 | `domain/constants.rs` |
| `MAX_QUEUE_PER_MODEL` | 2,000 | `domain/constants.rs` |
| `TIER_BONUS_PAID` | 300,000ms (300s) | `domain/constants.rs` |
| `TIER_BONUS_STANDARD` | 100,000ms (100s) | `domain/constants.rs` |
| Job cleanup TTL | 60s | `domain/constants.rs` |
| `MCP_LIFECYCLE_PHASE_FLAG_ENV` | `MCP_LIFECYCLE_PHASE` | `domain/constants.rs` |
| `MCP_LIFECYCLE_PHASE_DEFAULT` | `false` | `domain/constants.rs` |
| `LIFECYCLE_LOAD_TIMEOUT` | 600s | `infrastructure/outbound/ollama/lifecycle.rs` |
| `LIFECYCLE_STALL_INTERVAL` | 60s | `infrastructure/outbound/ollama/lifecycle.rs` |
| `LIFECYCLE_KEEP_ALIVE` | `30m` | `infrastructure/outbound/ollama/lifecycle.rs` |

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
