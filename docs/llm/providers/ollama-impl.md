# Providers -- Ollama: Streaming Protocol & Implementation

> SSOT | **Last Updated**: 2026-03-04 (rev: split from ollama.md)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change streaming dispatch logic | `ollama/adapter.rs` -- `stream_tokens()` |
| Change context length per model | `ollama/adapter.rs` -- `model_effective_num_ctx()` |
| Change generate request shape | `ollama/adapter.rs` -- `stream_generate()` |
| Change chat request shape | `ollama/adapter.rs` -- `stream_chat()` |
| Change format conversion (OpenAI) | `openai_handlers.rs` -- `ChatMessage::into_ollama_value()` |
| Change format conversion (Gemini) | `gemini_model_handlers.rs` -- `contents_to_ollama()` |
| Change done_reason handling | `ollama/adapter.rs` -- chunk filter in both stream functions |

## Key File

`crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` -- `OllamaAdapter`

---

## OllamaAdapter -- Streaming Protocol

`stream_tokens()` dispatches based on `job.messages`:

```rust
fn stream_tokens(&self, job: &InferenceJob) -> Pin<Box<dyn Stream<...>>> {
  if let Some(messages) = &job.messages {
    return self.stream_chat(job.model_name.as_str(), messages.clone());
  }
  self.stream_generate(job.model_name.as_str(), job.prompt.as_str())
}
```

| Condition | Endpoint | Used by |
|-----------|----------|---------|
| `job.messages = None` | `POST /api/generate` | `POST /v1/inference` (VeronexNative) |
| `job.messages = Some(...)` | `POST /api/chat` | All compat handlers (OpenAI, Ollama, Gemini) |

---

## Context Length (`num_ctx`) per Request

Every Ollama request includes `options.num_ctx` derived from `model_effective_num_ctx(model_name)`:

```rust
fn model_effective_num_ctx(model: &str) -> u32 {
  let m = model.to_lowercase();
  if m.contains("200k")                     { return 204_800; }
  if m.contains("128k")                     { return 131_072; }
  if m.contains("1m")                       { return 131_072; } // 1M models: 128K practical limit
  if m.contains("72b") || m.contains("70b") { return  32_768; }
  32_768 // default for 7B-32B models
}
```

This per-request override ensures each model uses its natural context window regardless of the global `OLLAMA_CONTEXT_LENGTH` env var on the Ollama server.

**Why this matters**: Without `options.num_ctx`, all models fall back to `OLLAMA_CONTEXT_LENGTH` (e.g. `8192`). A 128K model receiving a 24K-token conversation gets silently truncated, producing incomplete answers and triggering retry storms.

**Dual protection** (belt + suspenders):

| Layer | Mechanism |
|-------|-----------|
| GitOps | `OLLAMA_CONTEXT_LENGTH: 204800` on Ollama StatefulSet (global floor) |
| Veronex | `options.num_ctx` per request (model-specific override) |

---

## `/api/generate` -- Single Prompt

Request:
```json
{ "model": "qwen3:8b", "prompt": "...", "stream": true, "options": {"num_ctx": 32768} }
```

Response struct:
```rust
struct GenerateResponse {
  response: String,
  done: bool,
  done_reason: Option<String>,   // "stop" | "load" | "length"
  prompt_eval_count: Option<u32>,
  eval_count: Option<u32>,
}
```

---

## `/api/chat` -- Multi-Turn Messages

Request:
```json
{
  "model": "qwen3:8b",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user",   "content": "..."},
    {"role": "assistant", "content": "..."},
    {"role": "user",   "content": "..."}
  ],
  "stream": true,
  "options": {"num_ctx": 32768}
}
```

Response struct:
```rust
struct ChatChunk {
  message: Option<ChatChunkMessage>,  // { content: Option<String>, tool_calls: Option<Value> }
  done: bool,
  done_reason: Option<String>,
  prompt_eval_count: Option<u32>,
  eval_count: Option<u32>,
}
```

---

## `done_reason: "load"` Handling

When Ollama first loads a model into VRAM it emits an intermediate chunk with `done_reason: "load"`. Both `stream_generate()` and `stream_chat()` skip these chunks and keep reading. Without this fix, the stream terminates prematurely with empty output.

---

## Think Parameter — Not Used

The adapter does NOT set Ollama's `think` field on any request. Reasoning /
thinking behavior is a property of the Ollama model's own template — letting
Ollama decide per model keeps veronex's ReAct loop provider-agnostic and
avoids forcing a global policy that mis-fits some models
(e.g. `qwen3-coder` rejects `think:true` with HTTP 400; `qwen3` produces
empty output with `think:false` + large tool context).

The runner's `<think>…</think>` filter still strips any reasoning blocks
that models emit, so tokens counts may be inflated but the SSE content
never leaks internal reasoning to the client.

---

## Format Conversion (Compat Handlers to Ollama Messages)

| Entry route | Converter | Notes |
|-------------|-----------|-------|
| `POST /v1/chat/completions` | `ChatMessage::into_ollama_value()` | OpenAI `tool_calls[].arguments` (JSON string) to Ollama (JSON object) |
| `POST /api/chat` | Passthrough (already Ollama format) | -- |
| `POST /v1beta/models/*` | `contents_to_ollama()` | Gemini `role: "model"` to `"assistant"`, `functionCall`/`functionResponse` mapped |
| `POST /v1/test/*` | Passthrough or extract prompt | Test Run handlers pass simple messages or None |

---

## Related Documents

- **Provider registration, routing, health**: `docs/llm/providers/ollama.md`
- **Ollama model sync**: `docs/llm/providers/ollama-models.md`
- **Capacity / concurrency**: `docs/llm/inference/capacity.md`
