# Jobs — Token Observability & Analytics

> SSOT | **Last Updated**: 2026-03-03

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new column to `inference_logs` | `infrastructure/outbound/observability/` HttpObservabilityAdapter + `IngestInferenceRequest` struct | Extend `record_inference()` + veronex-analytics ingest endpoint |
| Change TTFT calculation logic | `infrastructure/outbound/provider_router.rs` `run_job()` | Modify TTFT detection block (first non-empty non-final token) |
| Add new analytics endpoint | `infrastructure/inbound/http/handlers.rs` + ClickHouse SQL | Add handler + route in `router.rs` |
| Change Ollama token count fallback | `infrastructure/outbound/provider_router.rs` `run_job()` | Modify `token_count` fallback (currently: SSE event count) |
| Change ClickHouse data retention | `docker/clickhouse/schema.sql` TTL clause | Modify `INTERVAL N DAY` on relevant table; for existing volumes use `ALTER TABLE ... MODIFY TTL` |
| Add Gemini prompt token tracking | `infrastructure/outbound/gemini/adapter.rs` `extract_usage()` | Already implemented — verify `prompt_tokens` flows to `emit_inference_event()` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/value_objects.rs` | `StreamToken` struct |
| `crates/veronex/src/infrastructure/outbound/gemini/adapter.rs` | `extract_usage()` — Gemini usageMetadata |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `run_job()` — TTFT + token recording |
| `crates/veronex/src/infrastructure/outbound/observability/` | `HttpObservabilityAdapter` (writes via veronex-analytics HTTP bridge) |
| `crates/veronex/src/application/ports/outbound/mod.rs` | `ObservabilityPort` trait |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | `/v1/usage`, `/v1/dashboard/performance` |

---

## StreamToken

```rust
// domain/value_objects.rs
pub struct StreamToken {
    pub value: String,
    pub is_final: bool,
    pub prompt_tokens: Option<u32>,     // set on last token only (Gemini)
    pub completion_tokens: Option<u32>, // None = provider didn't provide
}
```

---

## Gemini usageMetadata (gemini/adapter.rs)

Last SSE chunk (`finishReason: "STOP"`) contains:
```json
{ "usageMetadata": { "promptTokenCount": 12, "candidatesTokenCount": 85, "totalTokenCount": 97 } }
```

```rust
fn extract_usage(resp: &GenerateResponse) -> (Option<u32>, Option<u32>) {
    let u = resp.usage_metadata.as_ref();
    (u.and_then(|u| u.prompt_token_count), u.and_then(|u| u.candidates_token_count))
}
// → StreamToken { value: "", is_final: true, prompt_tokens, completion_tokens }
```

---

## run_job() — TTFT + Token Processing (provider_router.rs)

```
ttft_ms_value: Option<i32> = None

while token_stream.next():
    if token.prompt_tokens.is_some():
        actual_prompt_tokens = token.prompt_tokens
        actual_completion_tokens = token.completion_tokens

    // TTFT: first non-empty non-final token
    if ttft_ms_value.is_none() && !token.is_final && !token.value.is_empty():
        ttft_ms_value = now() - started_at

Save on completion:
    latency_ms        = completed_at - started_at
    ttft_ms           = ttft_ms_value
    completion_tokens = actual_completion_tokens ?? token_count

emit_inference_event(
    prompt_tokens     = actual_prompt_tokens.unwrap_or(0),
    completion_tokens = actual_completion_tokens.unwrap_or(token_count),
)
```

- **Gemini**: real counts from `usageMetadata`
- **Ollama**: SSE event count fallback (Ollama doesn't expose token counts in stream)

---

## ClickHouse inference_logs

```sql
CREATE TABLE inference_logs (
    job_id            String,
    api_key_id        Nullable(String),
    model_name        String,
    provider_type     String,
    status            String,
    prompt_tokens     UInt32,
    completion_tokens UInt32,
    latency_ms        Int32,
    created_at        DateTime
) ENGINE = MergeTree() ORDER BY created_at;
```

Written by `HttpObservabilityAdapter::record_inference()` via `POST /internal/ingest/inference` to veronex-analytics, which forwards to OTel Collector → Redpanda → ClickHouse.

---

## IngestInferenceRequest

```rust
pub struct IngestInferenceRequest {
    pub job_id:            String,
    pub api_key_id:        Option<String>,
    pub model_name:        String,
    pub provider_type:     String,   // "ollama" | "gemini"
    pub status:            String,
    pub prompt_tokens:     u32,
    pub completion_tokens: u32,
    pub latency_ms:        i32,
}
```

---

## Analytics Endpoints (handlers.rs)

```
GET /v1/usage?hours=
    → UsageAggregate { total_tokens, total_requests, by_model: Vec<ModelUsage> }
    (ClickHouse query, aggregate across all keys)

GET /v1/usage/{key_id}?hours=
    → Vec<HourlyUsage { hour, prompt_tokens, completion_tokens, request_count }>
    (per-key hourly breakdown)

GET /v1/dashboard/performance?hours=
    → PerformanceStats { avg_latency_ms, p50, p95, p99, success_rate, total_tokens,
                         hourly: Vec<HourlyThroughput> }
    (all from inference_logs ClickHouse table)
```

**SQL caution**: use subquery to avoid alias collision for `total_tokens` computation.

---

## DB Retention

```sql
-- PostgreSQL pg_cron: 90-day TTL
SELECT cron.schedule('0 3 * * *',
  'DELETE FROM inference_jobs WHERE created_at < now() - interval ''90 days''');

-- ClickHouse: optional training samples table
CREATE TABLE training_samples (
    id UUID, model_name String, prompt String CODEC(ZSTD(3)),
    result_text String CODEC(ZSTD(3)), quality_score Nullable(Float32), created_at DateTime
) ENGINE = MergeTree() PARTITION BY toYYYYMM(created_at) ORDER BY created_at;
```

---

## Gemini Streaming Bug Fix History

**Symptom**: `result_text = null` (final chunk not saved)

**Cause**: Last SSE event (`finishReason: "STOP"`) arrived without trailing `\n` → `buf` unprocessed

**Fix** (`gemini/adapter.rs`):
1. Flush remaining `buf` after byte stream ends
2. JSON parse failure → `warn!` log + skip (not error)
3. Final sentinel `StreamToken { value: "", is_final: true }` always emitted
