# Jobs — Repository Patterns & Cancellation

> SSOT | **Last Updated**: 2026-03-28
> Core lifecycle, entity, ZSET queue: `inference/job-lifecycle.md`

---

## JobRepository Patterns

```rust
// infrastructure/outbound/persistence/job_repository.rs (PostgresJobRepository)
save()            // INSERT ON CONFLICT DO NOTHING — initial Pending row, metadata + prompt_preview only
finalize()        // Single terminal UPDATE: status=completed, metrics, has_tool_calls
                  // Replaces the former mark_running + mark_completed two-step
cancel_job()      // UPDATE status=cancelled (early-exit path)
fail_with_reason()// UPDATE status=failed + failure_reason (queue-full / stream error)
get_status()      // in-memory DashMap first, DB fallback
```

**Write discipline**: Postgres sees exactly two writes per completed job — `save()` at submission and `finalize()` at stream end. No intermediate state is persisted.

### In-Memory Job Store (`InferenceUseCaseImpl`)

The live token buffer and job status are held in `Arc<DashMap<Uuid, JobEntry>>` (not Postgres):

```rust
// application/use_cases/inference/mod.rs
pub(crate) struct JobEntry {
    pub job: InferenceJob,
    pub status: JobStatus,
    pub tokens: Vec<StreamToken>,      // Vec::with_capacity(256) — avoids repeated realloc
    pub notify: Arc<Notify>,           // wakes stream() consumers on new token
    pub cancel_notify: Arc<Notify>,    // wakes run_job() cancel branch
    pub done: bool,
    pub api_key_id: Option<Uuid>,
    pub gemini_tier: Option<String>,
    pub key_tier: Option<KeyTier>,
    pub tpm_reservation_minute: Option<i64>, // minute bucket for TPM adjustment
    pub assigned_provider_id: Option<Uuid>,  // set at dispatch time (for Hard drain cancel)
}
```

**DashMap rule**: `Ref`/`RefMut` guards must be **dropped before any `.await`** — clone what you need, drop the guard, then await. Holding a guard across `.await` deadlocks the shard.

`PostgresJobRepository.save()` persists final state to DB only on completion/failure; intermediate tokens live only in the DashMap until the stream closes.

**Deferred cleanup**: `run_job` spawns a 60-second delayed `jobs.remove(&uuid)` on every exit path (completed, failed, cancelled). This prevents indefinite memory growth while keeping tokens replayable for late-connecting SSE clients.

---

## Cancellation

### API

```
DELETE /v1/dashboard/jobs/{id}   <- primary (JWT-protected, dashboard use)
DELETE /v1/inference/{job_id}    <- legacy alias (also wired)
    -> 200 OK  (idempotent — no-op if already terminal: Completed or Failed)
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
