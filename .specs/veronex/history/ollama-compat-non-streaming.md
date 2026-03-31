# Ollama API Compatibility — Non-Streaming Response SDD

> **Status**: In Progress | **Last Updated**: 2026-03-15
> **Branch**: feat/ollama-compat-non-streaming

---

## Problem

When Open-webui calls `/api/chat`, `/api/generate` with `stream: false`,
Veronex always responds with `application/x-ndjson` streaming.

Open-webui's internal tasks (title generation, tag generation, follow-up generation)
expect non-streaming JSON responses, causing parse failures → feature malfunction.

```
HTTPException: 200: Ollama: 200,
  message='Attempt to decode JSON with unexpected mimetype: application/x-ndjson',
  url='https://veronex-api.verobee.com/api/chat'
```

## Root Cause

`OllamaChatBody` and `OllamaGenerateBody` structs have no `stream` field.
→ `stream: false` is not deserialized, so the streaming path is always taken.

---

## Goal

Comply with Ollama API spec:
- `stream: true` (default) → existing `application/x-ndjson` streaming response
- `stream: false` → collect all tokens then return single `application/json` response

---

## Scope

| Endpoint | Handling |
|----------|------|
| `POST /api/chat` | Add stream field + implement non-streaming path |
| `POST /api/generate` | Add stream field + implement non-streaming path |

---

## Non-Streaming Response Format (Ollama official spec)

### `/api/chat` (stream: false) — plain text response

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "message": { "role": "assistant", "content": "<full text>" },
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

### `/api/chat` (stream: false) — tool call response

If any tool call tokens exist, `done_reason: "tool_calls"`, `message.content: ""`.

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "message": {
    "role": "assistant",
    "content": "",
    "tool_calls": [
      {
        "function": {
          "name": "get_weather",
          "arguments": { "location": "Seoul" }
        }
      }
    ]
  },
  "done_reason": "tool_calls",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

### `/api/generate` (stream: false)

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "response": "<full text>",
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

> Timing fields (`total_duration`, `load_duration`, `prompt_eval_duration`, `eval_duration`)
> are not measured by veronex, so fixed to `0`. Allowed by Ollama spec.

---

## StreamToken Structure (implementation reference)

`StreamToken` emitted by `OllamaAdapter`:

```rust
StreamToken {
    value: String,                          // token text
    is_final: bool,                         // true → last token
    prompt_tokens: Option<u32>,             // set only when is_final=true
    completion_tokens: Option<u32>,         // set only when is_final=true
    tool_calls: Option<serde_json::Value>,  // set on /api/chat tool call
}
```

Token processing in non-streaming path:
- `tool_calls.is_some()` → accumulate in `tool_calls_acc` (usually all in 1 token)
- `is_final` → extract `prompt_tokens`, `completion_tokens`
- remainder → `push_str` to `content`

---

## Implementation Plan

### Phase 1 — Struct modification (`ollama_compat_handlers.rs`)

```rust
// OllamaChatBody
#[serde(default)]
stream: Option<bool>,

// OllamaGenerateBody
#[serde(default)]
stream: Option<bool>,
```

### Phase 2 — `/api/chat` non-streaming path

`stream == Some(false)` branch:

```rust
if req.stream == Some(false) {
    let mut content = String::new();
    let mut tool_calls: Option<serde_json::Value> = None;
    let mut prompt_tokens = 0u32;
    let mut eval_tokens = 0u32;

    let mut token_stream = state.use_case.stream(&job_id);
    while let Some(result) = token_stream.next().await {
        match result {
            Ok(t) if t.tool_calls.is_some() => tool_calls = t.tool_calls,
            Ok(t) if t.is_final => {
                prompt_tokens = t.prompt_tokens.unwrap_or(0);
                eval_tokens   = t.completion_tokens.unwrap_or(0);
            }
            Ok(t) => content.push_str(&t.value),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": sanitize_sse_error(&e)}))).into_response(),
        }
    }

    let (done_reason, message) = if let Some(tc) = tool_calls {
        ("tool_calls", serde_json::json!({"role":"assistant","content":"","tool_calls": tc}))
    } else {
        ("stop", serde_json::json!({"role":"assistant","content": content}))
    };

    return Json(serde_json::json!({
        "model": model,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "message": message,
        "done_reason": done_reason,
        "done": true,
        "total_duration": 0, "load_duration": 0,
        "prompt_eval_count": prompt_tokens, "prompt_eval_duration": 0,
        "eval_count": eval_tokens, "eval_duration": 0,
    })).into_response();
}
// stream: true → existing ndjson path
```

### Phase 3 — `/api/generate` non-streaming path

Same pattern, `response` field instead of `message`:

```rust
Json(serde_json::json!({
    "model": model,
    "created_at": chrono::Utc::now().to_rfc3339(),
    "response": content,      // ← "response" not "message"
    "done_reason": "stop",
    "done": true,
    "total_duration": 0, "load_duration": 0,
    "prompt_eval_count": prompt_tokens, "prompt_eval_duration": 0,
    "eval_count": eval_tokens, "eval_duration": 0,
}))
```

> `/api/generate` has no tool calls, so tool_calls branching is unnecessary.

### Phase 4 — Testing

| Case | Verification |
|--------|----------|
| `stream: false` (chat) | `Content-Type: application/json`, `done: true`, `message.content` accumulated |
| `stream: false` (generate) | `response` field accumulated |
| `stream: false` + tool call | `done_reason: "tool_calls"`, `message.tool_calls` included |
| `stream: true` / unspecified | Existing `application/x-ndjson` behavior preserved |
| `stream: false` (no Ollama provider) | 503 returned |

---

## Tasks

| # | Task | File | Status |
|---|------|------|--------|
| 1 | Add `stream: Option<bool>` to `OllamaChatBody` / `OllamaGenerateBody` | `ollama_compat_handlers.rs` | **done** |
| 2 | Implement `/api/chat` non-streaming path (including tool_calls) | `ollama_compat_handlers.rs` | **done** |
| 3 | Implement `/api/generate` non-streaming path | `ollama_compat_handlers.rs` | **done** |
| 4 | Add tests (7 cases + proptest) | `ollama_compat_handlers.rs` | **done** |
