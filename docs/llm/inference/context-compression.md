# Context Compression

> SSOT | **Last Updated**: 2026-04-06 (rev 1 — initial)

Multi-turn context compression keeps long conversations within a model's context window. Implemented as three cooperating layers: per-turn async compression, context assembly, and session handoff.

---

## Overview

| Layer | When | What |
|-------|------|------|
| **Per-turn compression** (Phase 3) | After each turn completes | Compress completed turn via dedicated model; write summary to S3 |
| **Input compression** (Phase 5) | Before job submit | Compress long user prompt inline if it exceeds 50% of context budget |
| **Context assembly** (Phase 4) | Before job submit | Replace raw history with compressed summaries + recent verbatim window |
| **Session handoff** (Phase 6) | When total tokens > handoff_threshold × configured_ctx | Generate master summary, create new conversation, return new conversation_id |

Gated by `context_compression_enabled`. All layers fail-open — compression failure never blocks inference.

---

## Multi-Turn Eligibility Gate

Runs before context assembly. Returns `400` if any condition fails.

| Condition | LabSettings field | Default | Fail behavior |
|-----------|------------------|---------|---------------|
| Model ≥ N billion params | `multiturn_min_params` | 7 | `400 model_too_small` |
| Model max_ctx ≥ N tokens | `multiturn_min_ctx` | 16384 | `400 context_window_too_small` |
| Model in allowlist | `multiturn_allowed_models` | `[]` (all) | `400 model_not_allowed` |

`max_ctx` is read from Valkey (`veronex:ollama:ctx:{provider_id}:{model}`). Unknown → fail-open (allow).

Code: `context_assembler::check_multiturn_eligibility()` — `application/use_cases/inference/context_assembler.rs`

---

## Compression Router

Decides how to compress each completed turn. Code: `compression_router.rs`.

| Route | Condition |
|-------|-----------|
| `SyncInline` | Turn is short enough to compress in-request |
| `AsyncIdle` | Compress in background when provider is idle (N ≤ 3) |
| `AsyncDedicated` | Use dedicated compression model (`compression_model` set) |
| `Skip` | Compression disabled or turn too short to benefit |

`compression_model: None` → reuse the inference model.
`compression_timeout_secs` — max time for one compression call. Exceeded → skip silently.

---

## Pre-flight Pruning (gateway-side context budget gate)

**Trigger**: every LLM submission goes through `bridge::run_loop`'s entry, which resolves the model's effective budget and trims older messages if the accumulated array exceeds it.

| Step | Mechanism |
|---|---|
| Resolve ctx | `ModelCapacityRepository::min_configured_ctx_for_model(model)` — returns the smallest `configured_ctx` across all serving providers (most conservative). Falls back to `32_768` if no profile row exists. |
| Compute budget | `context_budget::budget_for_context(ctx, ratio)` — `ctx × lab_settings.context_budget_ratio` (default 0.60), minus a 1024-tok safety margin for cl100k_base ±10% drift on Qwen/Llama tokenizers. |
| Token count | `context_budget::count_messages_tokens(messages)` — `tiktoken-rs` `cl100k_base` over role + content + 4-tok per-message scaffolding + 2 priming. |
| Trim | `context_pruner::prune_to_budget(messages, budget, recent_k)` — drops oldest non-system messages until under budget; preserves system + last `recent_k` (=5) verbatim; inserts ONE placeholder system message marking the omission. |

**DCP invariant**: pruning is in-memory only. The S3 ConversationRecord is **never** modified — every original turn is retained for dashboard / audit / replay.

**Behavior on overflow**: pre-fix this ladder didn't exist (`configured_ctx = 32_768u32` hardcoded in `ollama_compat_handlers.rs`). Long MCP loops (30+ rounds) would grow `messages[]` unbounded → context overflow at provider level. Post-fix: bridge log emits structured event `context-pruner: trimmed accumulated messages to fit budget {model, configured_ctx, budget, initial_tokens, after_tokens, dropped}` whenever trim fires.

**Live verified 2026-04-29** (image `develop-921771c`): synthetic 30-turn `qwen3:8b` conversation (27,394 initial tokens; budget 18,636) trimmed to 18,592 tokens, 19 messages dropped; model answered the final user query correctly (system + last 5 turns preserved).

SDD: `.specs/veronex/history/conversation-context-compression.md`.

---

## Per-Turn Compression

Code: `context_compressor::compress_turn()` — `application/use_cases/inference/context_compressor.rs`

1. Build compression prompt (system prompt: lossless summarizer, ≤120 words)
2. Call Ollama `/api/chat` with compression model
3. On success: rewrite `TurnRecord.compressed` in S3 with `CompressedTurn { summary, compression_model, original_tokens, compressed_tokens, ratio }`
4. Invalidate Valkey conversation cache (`DEL veronex:conv:{id}`)
5. On failure: log warn, leave raw turn in S3 (fail-open)

Triggered from `runner.rs` via `tokio::spawn` after job finishes — does not block the response.
Trigger interval: `compression_trigger_turns` (default 1 = every turn).

---

## Input Compression (Inline)

Code: `context_compressor::compress_input_inline()`

If the latest user message exceeds 50% of context budget (`configured_ctx × context_budget_ratio × 0.5`), compress it before submission. Replaces `last_user` message content in the outgoing Ollama messages array.

Applied in `openai_handlers.rs` and `ollama_compat_handlers.rs` after Ollama message conversion.

---

## Context Assembly

Code: `context_assembler::assemble()` — `context_assembler.rs`

Called before each multi-turn job submit. Replaces raw `messages` with assembled context.

Assembly order (oldest → newest):
1. For each historical turn: prefer `compressed.summary` if available, else raw content
2. Keep last `recent_verbatim_window` turns uncompressed (raw text)
3. Enforce budget: drop oldest messages until total ≤ `configured_ctx × context_budget_ratio`

Token estimation: `chars / 4` (rough — accurate enough for budget enforcement).

---

## Session Handoff

Code: `session_handoff.rs`

Triggered when `sum(compressed_tokens across all turns) > handoff_threshold × configured_ctx`.

Handoff sequence:
1. `generate_master_summary()` — call compression model with all compressed summaries → master summary
2. Create new `ConversationRecord` in S3 with a `HandoffTurn { master_summary, original_conversation_id }`
3. New conversation ID returned to client via `X-Conversation-ID` response header and `conversation_renewed: true` in response body
4. On failure: log warn, continue with original conversation ID (fail-open)

Fields:
- `handoff_enabled: bool` — default `true`
- `handoff_threshold: f32` — default `0.85`

---

## Data Model

```rust
// application/ports/outbound/message_store.rs
pub struct CompressedTurn {
    pub summary: String,
    pub compression_model: String,
    pub original_tokens: u32,
    pub compressed_tokens: u32,
    pub ratio: f32,
}

pub struct HandoffTurn {
    pub master_summary: String,
    pub original_conversation_id: Uuid,
    pub handoff_at: DateTime<Utc>,
}
```

`TurnRecord.compressed: Option<CompressedTurn>` — None until compression completes.
`TurnRecord.vision_analysis: Option<VisionAnalysis>` — vision model analysis result.

---

## Response Fields

Non-streaming `ChatCompletion` and Ollama `/api/chat` responses include:

| Field | Type | Notes |
|-------|------|-------|
| `conversation_id` | `string \| null` | Omitted if no conversation context |
| `conversation_renewed` | `bool` | Only present (true) when handoff created a new session |

`X-Conversation-ID` response header — always set when conversation context is active.

---

## Frontend

`TurnInternals` component (`web/components/turn-internals.tsx`) — collapsible panel per turn showing compression stats, vision analysis, **and MCP tool-call audit** (`tool_calls` array, when the turn invoked any MCP tool — name/args/result/outcome/latency per round). Lazy-fetches via `GET /v1/dashboard/conversations/{id}/turns/{job_id}/internals`. MCP audit shape and source documented in `inference/mcp.md` § Audit exposure; SDD `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md`.

Context warning badge — `api-test-form.tsx` `getMultiturnWarnings()` fires `context_too_large` when estimated conversation tokens exceed 85% of model's `max_ctx`. Uses real `max_ctx` from API when profiled, heuristic fallback otherwise.

---

## Valkey Keys

| Key | Value | TTL | Written by |
|-----|-------|-----|-----------|
| `veronex:conv:{conversation_id}` | S3-cached conversation record | 7d | S3 write-through |
| `veronex:ollama:ctx:{provider_id}:{model}` | `{"configured_ctx": u32, "max_ctx": u32}` | 600s | capacity analyzer |

→ See `infra/distributed.md`, `infra/hot-path-caching.md`
→ Flow: `flows/context-compression.md`
