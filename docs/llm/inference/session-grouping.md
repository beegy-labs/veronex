# Session Grouping & Training Data

> SSOT | **Last Updated**: 2026-03-03

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

---

## Conversation Threading

Two mechanisms assign `conversation_id`:

| Method | Behavior | Target Clients |
|--------|----------|----------------|
| `X-Conversation-ID` header | Client sends UUID directly, applied immediately | Cline, Claude Code, Gemini CLI (header-capable) |
| Server batch auto-inference | `run_session_grouping_loop` (daily) — assigns when absent | Qwen Code, Cursor (no header support) |

---

## Batch Auto-Inference Algorithm

Implementation: `infrastructure/outbound/session_grouping.rs`

- On job save: compute `messages_hash` + `messages_prefix_hash` via Blake2b-256
- `messages_prefix_hash` = hash(messages[0..-1]) — excludes last user message
- Batch: job B's `messages_prefix_hash` == job A's `messages_hash` → same `conversation_id`
- First turn (`messages_prefix_hash = ""`): new `conversation_id` (UUIDv7) created
- `SESSION_GROUPING_INTERVAL_SECS` env var controls interval (default: 86400s = 24h)
- No race conditions — based on completed job history, no LLM calls
- **Date cutoff**: `created_at < DATE_TRUNC('day', NOW())` — ongoing conversations untouched
- **Concurrency guard**: `session_grouping_lock: Arc<Semaphore(1)>` (AppState) — shared by batch + manual trigger
- **No time upper bound** — groups data older than 1 week too; LIMIT 10000 for daily incremental processing

### DB Indexes (session grouping)

```sql
CREATE INDEX idx_inference_jobs_messages_hash
    ON inference_jobs(api_key_id, messages_hash)
    WHERE messages_hash IS NOT NULL;
CREATE INDEX idx_inference_jobs_session_ungrouped
    ON inference_jobs(api_key_id, messages_prefix_hash, created_at)
    WHERE conversation_id IS NULL AND messages_prefix_hash IS NOT NULL AND messages_prefix_hash != '';
```

---

## Manual Trigger

`POST /v1/dashboard/session-grouping/trigger` (JWT Bearer):

```json
// Request (optional before_date — defaults to today midnight)
{ "before_date": "2026-03-01" }

// 202 Accepted — runs in background, returns immediately
{ "message": "session grouping triggered" }

// 409 Conflict — already running
{ "message": "session grouping already in progress" }
```

**Public function**: `group_sessions_before(pg_pool, cutoff: Option<NaiveDate>)` — called by both handler and batch loop.

---

## Related Docs

- Job entity & lifecycle: `docs/llm/inference/job-lifecycle.md`
- Frontend session UI: `docs/llm/frontend/pages/jobs.md` — GroupSessionsPanel
- Job analytics: `docs/llm/inference/job-analytics.md`
