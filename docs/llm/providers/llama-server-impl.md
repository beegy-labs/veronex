# Providers -- llama-server: Streaming Protocol & Implementation

> SSOT | **Last Updated**: 2026-03-04 (rev: split from llama-server.md)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change streaming dispatch logic | `llama_server/adapter.rs` -- `stream_tokens()` |
| Change context length per model | `llama_server/adapter.rs` -- `model_effective_num_ctx()` |
| Change generate request shape | `llama_server/adapter.rs` -- `stream_generate()` |
| Change chat request shape | `llama_server/adapter.rs` -- `stream_chat()` |
| Change format conversion (OpenAI) | `openai_handlers.rs` -- `ChatMessage::into_chat_value()` |
| Change format conversion (Gemini) | `gemini_model_handlers.rs` -- `contents_to_messages()` |
| Change done_reason handling | `llama_server/adapter.rs` -- chunk filter in both stream functions |

## Key File

`crates/veronex/src/infrastructure/outbound/llama_server/adapter.rs` -- `LlamaServerAdapter`

---

## LlamaServerAdapter -- Streaming Protocol

`stream_tokens()` dispatches based on `job.messages`:

```rust
fn stream_tokens(&self, job: &InferenceJob) -> Pin<Box<dyn Stream<...>>> {
  if let Some(messages) = &job.messages {
    return self.stream_chat(job.model_name.as_str(), messages.clone());
  }
  self.stream_generate(job.model_name.as_str(), job.prompt.as_str())
}
```

All inference paths funnel into a single upstream endpoint:

| Condition | Upstream call | Used by |
|-----------|---------------|---------|
| `job.messages = None` | `POST {base}/v1/chat/completions` (single user-role message synthesised from `job.prompt`) | `POST /v1/inference` (VeronexNative) |
| `job.messages = Some(...)` | `POST {base}/v1/chat/completions` (messages forwarded as-is) | OpenAI compat + Gemini compat |

---

## Context Length (`num_ctx`) per Request

**Modelfile is the SSOT.** With the legacy `capacity::analyzer` removed,
`num_ctx` per model is derived from the `Modelfile` registry row (Phase 2):

- Postgres `modelfiles.context_length` (canonical)
- Valkey `model_ctx(provider_id, model)` (TTL 600 s, hot-path cache,
  populated lazily from the Modelfile row on first use)

**Every request to llama-server (Phase 1 lifecycle probe AND Phase 2 inference) MUST send the same `options.num_ctx`** resolved through the same lookup chain:

```rust
pub async fn resolve_num_ctx(pool, provider_id, model) -> u32 {
    lookup_ctx(pool, provider_id, model)        // 1. Valkey (sync SSOT)
        .await
        .unwrap_or_else(|| model_effective_num_ctx(model))   // 2. fabricate fallback
}

// fabricate values MUST match what sync would store for that model
fn model_effective_num_ctx(model: &str) -> u32 {
  let m = model.to_lowercase();
  if m.contains("200k")                     { return 200_000; }   // Modelfile 200000
  if m.contains("128k")                     { return 131_072; }
  if m.contains("1m")                       { return 131_072; }
  if m.contains("72b") || m.contains("70b") { return  32_768; }
  32_768
}
```

**Why one-source matters — single runner per model**:

llama-server's scheduler (`LLAMA_SERVER_NUM_PARALLEL=1`) treats the **same model with different `KvSize`** as separate runner subprocesses. If Phase 1 probe sends a different `num_ctx` than Phase 2 chat, llama-server spawns a **second cold-load** for the second `KvSize`. This breaks the "model loaded once, AIMD-tuned concurrent jobs" invariant of the queue+dispatcher design (`docs/llm/inference/capacity.md`, `docs/llm/providers/llama-server-allocation.md`). Verified 2026-04-30 on dev: 220 + 232 s instead of 220 s.

The fabricate fallback exists for the cold-start window before the analyzer's first sync. Its values MUST equal what sync would return — drift between fabricate (e.g. `204_800`) and Modelfile (`200_000`) reproduces the double-runner problem within a single request when one path hits Valkey and the other misses.

**Layered protection**:

| Layer | Mechanism | Role |
|-------|-----------|------|
| GitOps | `LLAMA_SERVER_CONTEXT_LENGTH: 204800` on llama-server StatefulSet | Server-wide floor (used only when client sends no `num_ctx`) |
| Modelfile registry (SSOT) | `modelfiles.context_length` → Valkey `model_ctx` | Canonical per-model value from the Phase 2 registry |
| Veronex fabricate (fallback) | `model_effective_num_ctx` name-pattern | Cold-start guess; values aligned to Modelfile conventions |

SDD: `.specs/veronex/lifecycle-num-ctx-ssot-alignment.md`.

---

## `POST /v1/chat/completions` (OpenAI-compat)

Request — same chat-completion shape clients send to OpenAI:
```json
{
  "model": "qwen3:8b",
  "messages": [
    {"role": "system",    "content": "..."},
    {"role": "user",      "content": "..."},
    {"role": "assistant", "content": "..."},
    {"role": "user",      "content": "..."}
  ],
  "stream": true,
  "max_tokens": 4096,
  "temperature": 0.7
}
```

Streaming response: SSE `data:` frames carrying OpenAI-style chunks
(`choices[].delta.content`, `choices[].delta.tool_calls`, etc.). Final
frame is `data: [DONE]`. Veronex consumes the SSE on the gateway side and
re-emits to API-key clients without re-buffering.

The legacy llama-server-native `/api/generate` (single-prompt, NDJSON) and
`/api/chat` (multi-turn, NDJSON with `done_reason: "load"` warmup chunks)
were removed in the migration off Ollama-style endpoints.

---

## Think Parameter — Not Used

The adapter does NOT set llama-server's `think` field on any request. Reasoning /
thinking behavior is a property of the llama-server model's own template — letting
llama-server decide per model keeps veronex's MCP loop provider-agnostic and
avoids forcing a global policy that mis-fits some models
(e.g. `qwen3-coder` rejects `think:true` with HTTP 400; `qwen3` produces
empty output with `think:false` + large tool context).

The runner's `<think>…</think>` filter still strips any reasoning blocks
that models emit, so tokens counts may be inflated but the SSE content
never leaks internal reasoning to the client.

---

## Format Conversion (Compat Handlers to llama-server Messages)

| Entry route | Converter | Notes |
|-------------|-----------|-------|
| `POST /v1/chat/completions` | `ChatMessage::into_chat_value()` | Normalises content (text vs. parts) and rewrites OpenAI `tool_calls[].arguments` (JSON string) into the object form llama-server accepts |
| `POST /v1beta/models/*` | `contents_to_messages()` | Gemini `role: "model"` → `"assistant"`, `functionCall` / `functionResponse` mapped to chat `tool_calls` / `tool` |
| `POST /v1/inference` | Synthesises a single `user` message from `prompt` | VeronexNative entry — no message history |

---

## Related Documents

- **Provider registration, routing, health**: `docs/llm/providers/llama-server.md`
- **Modelfile registry / install pipeline**: `docs/llm/providers/llama-server-models.md`
- **Capacity / concurrency**: `docs/llm/inference/capacity.md`
- **ProcessManager + IdleManager lifecycle**: `docs/llm/flows/process-manager.md`
