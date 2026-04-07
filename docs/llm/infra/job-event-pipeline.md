# Job Write Pipeline

> CDD Layer 2 | **Last Updated**: 2026-03-26

## Overview

Minimal-write architecture for `inference_jobs`.
Postgres stores metadata only; large content (prompt, messages, tool_calls, result) goes to S3.

---

## Design Principles

| Principle | Details |
|-----------|---------|
| **2 writes** | `save()` = initial INSERT, `finalize()` = single terminal UPDATE. 2 Postgres writes per completed job |
| **S3 content** | ConversationRecord (prompt + messages + tool_calls + result) -> zstd-3 compressed JSON, 1 S3 PUT |
| **Early exit** | `cancel_job()` / `fail_with_reason()` for early-exit only (queue full, stream error) |
| **Single truth** | Postgres = metadata SSOT, S3 = content SSOT |

---

## JobRepository Call Map

| Call site | Method | Target |
|-----------|--------|--------|
| `submit()` | `save()` | Postgres INSERT (sync) |
| `submit()` queue full | `fail_with_reason()` | Postgres UPDATE (sync) |
| `finalize_job()` | S3 PUT + `finalize()` | S3 (non-fatal) + Postgres UPDATE |
| `cancel()` | `cancel_job()` | Postgres UPDATE (sync) |
| `handle_stream_error()` | `fail_with_reason()` | Postgres UPDATE (sync) |
| Restart recovery | `list_pending()` | Postgres SELECT |
| Status query miss | `get()` | Postgres SELECT |

---

## S3 ConversationRecord

Key pattern: `conversations/{owner_id}/{YYYY-MM-DD}/{job_id}.json.zst`

```rust
pub struct ConversationRecord {
    pub prompt: String,
    pub messages: Option<serde_json::Value>,   // full conversation context
    pub tool_calls: Option<serde_json::Value>, // includes both MCP + OpenAI function calls
    pub result: Option<String>,                // final text output
}
```

- `owner_id = account_id ?? api_key_id ?? job_id`
- zstd-3 compression (~1.2 KB / record, ~8-11x vs original)
- Single PUT at `finalize_job()` time (non-fatal -- warns and continues on S3 failure)
- Single GET on admin detail view click (~20-50ms)

---

## `finalize()` Parameters

```rust
async fn finalize(
    job_id, started_at, completed_at,
    provider_id, queue_time_ms,
    latency_ms, ttft_ms,
    prompt_tokens, completion_tokens, cached_tokens,
    has_tool_calls: bool,   // true if tool_calls exist in S3 -- for list view display
) -> Result<()>
```

---

## Postgres Columns (inference_jobs)

Metadata only -- large columns removed:

| Removed | Replacement |
|---------|-------------|
| `prompt` (full) | `prompt_preview VARCHAR(200)` (for list/search) |
| `result_text` | S3 `ConversationRecord.result` |
| `messages_json` | S3 `ConversationRecord.messages` |
| `tool_calls_json` | S3 `ConversationRecord.tool_calls` + `has_tool_calls BOOLEAN` |

---

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `S3_ENDPOINT` | required | MinIO/S3 endpoint |
| `S3_BUCKET` | `veronex` | Bucket name |
| `S3_ACCESS_KEY` | required | Credentials |
| `S3_SECRET_KEY` | required | Credentials |

---

## Key Files

| File | Role |
|------|------|
| `crates/veronex/src/application/ports/outbound/job_repository.rs` | `JobRepository` trait -- defines `save()`, `finalize()` |
| `crates/veronex/src/application/ports/outbound/message_store.rs` | `MessageStore` trait -- `ConversationRecord`, S3 read/write |
| `crates/veronex/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` implementation |
| `crates/veronex/src/infrastructure/outbound/s3/message_store.rs` | `S3MessageStore` implementation (zstd-3) |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | `finalize_job()` -- S3 PUT + DB finalize call site |

---

## Task Guide

| Task | File |
|------|------|
| Add ConversationRecord field | `message_store.rs` struct + S3 impl |
| Add finalize metric | `job_repository.rs` port + infra + `runner.rs` call site |
| Change search scope | `dashboard_queries.rs` `fetch_jobs` -- `prompt_preview ILIKE` |
| Change S3 key pattern | `s3/message_store.rs` `key()` function |
| Full flow | See `docs/llm/flows/job-event-pipeline.md` |
