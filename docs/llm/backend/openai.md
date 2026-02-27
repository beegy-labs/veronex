# OpenAI-Compatible Inference API

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new `backend` field value (e.g. `"anthropic"`) | `openai_handlers.rs` `chat_completions()` + `backend_router.rs` dispatch | Add new arm to `backend` match |
| Support multi-turn history forwarding | `openai_handlers.rs` `chat_completions()` | Change "last user message only" extraction to full messages |
| Change SSE chunk format (OpenAI delta) | `openai_handlers.rs` SSE emit block | Modify `ChatCompletionChunk` serialization |
| Add new field to ChatCompletionRequest | `openai_handlers.rs` `ChatCompletionRequest` struct | Add field + propagate to `submit()` call |
| Update OpenAPI spec | `infrastructure/inbound/http/openapi.json` | Edit JSON directly — embedded via `include_str!` at build time |
| Change /docs auth (require auth) | `router.rs` | Move `/docs/*` routes inside auth middleware layer |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions` handler |
| `crates/inferq/src/infrastructure/inbound/http/handlers.rs` | Native `/v1/inference` handlers |
| `crates/inferq/src/infrastructure/inbound/http/docs_handlers.rs` | Swagger / ReDoc / OpenAPI spec |
| `crates/inferq/src/infrastructure/inbound/http/router.rs` | Route registration |
| `crates/inferq/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl::submit()` |
| `crates/inferq/src/infrastructure/inbound/http/openapi.json` | OpenAPI 3.0.3 spec (embedded via `include_str!`) |

---

## POST /v1/chat/completions

**Auth**: `X-API-Key` header. Streams SSE always (the `stream` field is ignored).

### Request Struct

```rust
// openai_handlers.rs
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub backend: Option<String>, // "ollama" | "gemini-free" | "gemini" (default: "ollama")
    pub stream: Option<bool>,    // ignored — always streamed
}

pub struct ChatMessage {
    pub role: String,    // "system" | "user" | "assistant"
    pub content: String,
}
```

**Note**: Only the last `user` message is used as the inference prompt.
Multi-turn history is NOT forwarded to backends.

### `backend` Field

| Value | Routing |
|-------|---------|
| `"ollama"` (default) | VRAM-aware Ollama selection |
| `"gemini-free"` | `is_free_tier=true` only — no paid fallback |
| `"gemini"` | Auto: free-first → paid fallback on RPD exhaustion |

### SSE Response

```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}

data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

### Error Response

```json
{ "error": { "message": "no active ollama backends", "type": "internal_error" } }
```

---

## Native Inference Endpoints

```
POST   /v1/inference              Submit job → { job_id }
GET    /v1/inference/{id}/stream  SSE token stream
GET    /v1/inference/{id}/status  { status, backend, model_name, … }
DELETE /v1/inference/{id}         Cancel (idempotent)
```

Native + OpenAI endpoints share the same queue and job lifecycle.
→ See `docs/llm/backend/jobs.md` for job lifecycle.

### Native Request

```json
{ "model": "llama3.2", "prompt": "Hello!", "backend": "ollama" }
```

---

## API Documentation Endpoints

Served by `docs_handlers.rs` — no authentication required:

```
GET /docs/openapi.json   OpenAPI 3.0.3 spec (embedded via include_str!)
GET /docs/swagger        Swagger UI 5 (unpkg CDN)
GET /docs/redoc          ReDoc latest (jsDelivr CDN)
```

Web page `/api-docs` links to all three. → See `docs/llm/frontend/web-test.md`.

---

## SSE Parsing Rules

- Strip one leading space after `data:` — preserve internal whitespace
- `data: [DONE]` → stream complete
- Chunk may have `finish_reason: "stop"` before `[DONE]`
- Error chunks: `{ "error": { "message": "...", "type": "internal_error" } }`

---

## Client Examples

### curl

```bash
curl http://localhost:3001/v1/chat/completions \
  -H "X-API-Key: veronex-bootstrap-admin-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2","messages":[{"role":"user","content":"Hello"}],"backend":"ollama"}'
```

### OpenAI Python SDK

```python
from openai import OpenAI
client = OpenAI(api_key="key", base_url="http://localhost:3001/v1",
                default_headers={"X-API-Key": "key"})
stream = client.chat.completions.create(
    model="llama3.2",
    messages=[{"role":"user","content":"Hello"}],
    stream=True, extra_body={"backend":"ollama"},
)
for chunk in stream:
    print(chunk.choices[0].delta.content, end="")
```
