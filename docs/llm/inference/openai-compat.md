# OpenAI-Compatible Inference API

> SSOT | **Last Updated**: 2026-03-04

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new `provider_type` value (e.g. `"anthropic"`) | `openai_handlers.rs` `chat_completions()` + `provider_router.rs` dispatch | Add new arm to `provider_type` match |
| Change SSE chunk format (OpenAI delta) | `openai_sse_types.rs` | Modify `CompletionChunk` / `DeltaContent` structs |
| Add new field to ChatCompletionRequest | `openai_handlers.rs` `ChatCompletionRequest` struct | Add field + propagate to `submit()` call |
| Update OpenAPI spec | `infrastructure/inbound/http/openapi.json` | Edit JSON directly — embedded via `include_str!` at build time |
| Change /docs auth (require auth) | `router.rs` | Move `/docs/*` routes inside auth middleware layer |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions` handler + Ollama proxy + legacy queue path |
| `crates/veronex/src/infrastructure/inbound/http/openai_sse_types.rs` | Shared SSE types: `CompletionChunk`, `DeltaContent`, `ChunkChoice` |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | Native `/v1/inference` handlers |
| `crates/veronex/src/infrastructure/inbound/http/docs_handlers.rs` | Swagger / ReDoc / OpenAPI spec |
| `crates/veronex/src/infrastructure/inbound/http/router.rs` | Route registration |
| `crates/veronex/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl::submit()` |
| `crates/veronex/src/infrastructure/inbound/http/openapi.json` | OpenAPI 3.0.3 spec (embedded via `include_str!`) |

---

## POST /v1/chat/completions

**Auth**: `X-API-Key` header (also accepts `Authorization: Bearer` and `x-goog-api-key`).
Streams SSE always (the `stream` field is ignored).

### Request Struct

```rust
// openai_handlers.rs
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub provider_type: Option<String>,          // "ollama" | "gemini-free" | "gemini" (default: "ollama")
    pub tools: Option<Vec<serde_json::Value>>,  // tool/function definitions (passed to Ollama)
    pub tool_choice: Option<serde_json::Value>, // tool choice override
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<u32>,                // maps to Ollama options.num_predict
    pub stream: Option<bool>,                   // ignored — always streamed
}

pub struct ChatMessage {
    pub role: String,                                // "system" | "user" | "assistant" | "tool"
    pub content: Option<MessageContent>,             // String or [{type, text}] content parts
    pub tool_calls: Option<serde_json::Value>,       // assistant tool-call messages
    pub tool_call_id: Option<String>,                // tool result correlation ID
    pub name: Option<String>,                        // tool result name
}
```

### Two Execution Paths

| `provider_type` | Path | Behavior |
|-----------------|------|----------|
| `"ollama"` (default) | **Ollama proxy** | Full conversation history forwarded. Messages converted to Ollama `/api/chat` format. Tools, temperature, top_p, max_tokens passed through. Tool call arguments: OpenAI JSON string → Ollama JSON object. |
| anything else (`"gemini-free"`, `"gemini"`) | **Legacy queue** | Only the last `user` message extracted as prompt. Enqueued via Valkey queue, single-prompt inference. |

### `provider_type` Field

| Value | Routing |
|-------|---------|
| `"ollama"` (default) | VRAM-aware Ollama selection |
| `"gemini-free"` | `is_free_tier=true` only — no paid fallback |
| `"gemini"` | Auto: free-first → paid fallback on RPD exhaustion |

> **Note**: `"gemini-free"` is not a `ProviderType` enum variant — it maps to `ProviderType::Gemini` with `tier_filter = Some("free")`. The `ProviderType` enum has only two variants: `Ollama` and `Gemini`.

### SSE Response

Content tokens:
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
```

Tool calls (Ollama function calling):
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_0","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Seoul\"}"}}]},"finish_reason":null}]}
```

Stop/finish:
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

`finish_reason` is `"tool_calls"` when the model returned tool calls, `"stop"` otherwise.

### Error Response

```json
{ "error": { "message": "failed to submit inference job" } }
```

**429 Too Many Requests**: Returned with `Retry-After: 60` header when all Gemini providers are rate-limited. The client should wait and retry after the specified interval.

### Input Validation

- Model name: max 256 bytes (`MAX_MODEL_NAME_BYTES` in `constants.rs`)
- Total message content: max 1MB (`MAX_PROMPT_BYTES` in `constants.rs`)
- Validated per API format (native, OpenAI, Gemini, Ollama) before processing

---

## Native Inference Endpoints

```
POST   /v1/inference              Submit job → { job_id }
GET    /v1/inference/{id}/stream  SSE token stream (event: token / done / error)
GET    /v1/inference/{id}/status  { job_id, status }
DELETE /v1/inference/{id}         Cancel (idempotent)
GET    /v1/jobs/{id}/stream       OpenAI-format SSE replay (for reconnect)
```

Native + OpenAI endpoints share the same queue and job lifecycle.
→ See `docs/llm/inference/job-lifecycle.md` for job lifecycle.

### Native Request

```json
{ "prompt": "Hello!", "model": "llama3.2", "provider_type": "ollama" }
```

### Status Response

```json
{ "job_id": "019…", "status": "running" }
```

---

## API Documentation Endpoints

Served by `docs_handlers.rs` — no authentication required:

```
GET /docs/openapi.json   OpenAPI 3.0.3 spec (embedded via include_str!)
GET /docs/swagger        Swagger UI 5 (unpkg CDN)
GET /docs/redoc          ReDoc latest (jsDelivr CDN)
```

Web page `/api-docs` links to all three. → See `docs/llm/frontend/pages/api-test.md`.

---

## SSE Parsing Rules

- Strip one leading space after `data:` — preserve internal whitespace
- `data: [DONE]` → stream complete
- Chunk may have `finish_reason: "stop"` or `"tool_calls"` before `[DONE]`
- Error chunks: `{ "error": { "message": "..." } }`

---

## Client Examples

### curl

```bash
curl http://localhost:3001/v1/chat/completions \
  -H "X-API-Key: veronex-bootstrap-admin-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2","messages":[{"role":"user","content":"Hello"}],"provider_type":"ollama"}'
```

### OpenAI Python SDK

```python
from openai import OpenAI
client = OpenAI(api_key="key", base_url="http://localhost:3001/v1",
                default_headers={"X-API-Key": "key"})
stream = client.chat.completions.create(
    model="llama3.2",
    messages=[{"role":"user","content":"Hello"}],
    stream=True, extra_body={"provider_type":"ollama"},
)
for chunk in stream:
    print(chunk.choices[0].delta.content, end="")
```
