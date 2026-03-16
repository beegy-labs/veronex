# OpenAI-Compatible Inference API

> SSOT | **Last Updated**: 2026-03-15

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new `provider_type` value (e.g. `"anthropic"`) | `openai_handlers.rs` `chat_completions()` + `provider_router.rs` dispatch | Add new arm to `provider_type` match |
| Change SSE chunk format (OpenAI delta) | `openai_sse_types.rs` | Modify `CompletionChunk` / `DeltaContent` structs |
| Add new field to ChatCompletionRequest | `openai_handlers.rs` `ChatCompletionRequest` struct | Add field + propagate to `submit()` call |
| Update OpenAPI spec | `infrastructure/inbound/http/openapi.json` | Edit JSON directly — embedded via `include_str!` at build time |
| Change /docs auth (require auth) | `router.rs` | Move `/docs/*` routes inside auth middleware layer |
| Add a new OpenAI-compat stub endpoint (501) | `openai_media_handlers.rs` | Add async fn + register in `router.rs` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions` handler + Ollama proxy + legacy queue path |
| `crates/veronex/src/infrastructure/inbound/http/openai_sse_types.rs` | Shared SSE/response types: `CompletionChunk`, `DeltaContent`, `ChatCompletion`, `StreamOptions`, `SYSTEM_FINGERPRINT` |
| `crates/veronex/src/infrastructure/inbound/http/openai_completions_handlers.rs` | `POST /v1/completions` — legacy text completions |
| `crates/veronex/src/infrastructure/inbound/http/openai_embeddings_handlers.rs` | `POST /v1/embeddings` — OpenAI-compat embeddings (proxies to Ollama /api/embed) |
| `crates/veronex/src/infrastructure/inbound/http/openai_models_handlers.rs` | `GET /v1/models`, `GET /v1/models/{model_id}` — model listing |
| `crates/veronex/src/infrastructure/inbound/http/openai_media_handlers.rs` | `POST /v1/audio/*`, `/v1/images/generations`, `/v1/moderations` — 501 stubs |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | Native `/v1/inference` handlers |
| `crates/veronex/src/infrastructure/inbound/http/docs_handlers.rs` | Swagger / ReDoc / OpenAPI spec |
| `crates/veronex/src/infrastructure/inbound/http/router.rs` | Route registration |
| `crates/veronex/src/application/use_cases/inference.rs` | `InferenceUseCaseImpl::submit()` |
| `crates/veronex/src/infrastructure/inbound/http/openapi.json` | OpenAPI 3.0.3 spec (embedded via `include_str!`) |

---

## Endpoint Summary

| Method | Path | Handler file | Notes |
|--------|------|--------------|-------|
| POST | `/v1/chat/completions` | `openai_handlers.rs` | Streaming + non-streaming; Ollama proxy + Gemini legacy path |
| POST | `/v1/completions` | `openai_completions_handlers.rs` | Legacy text completion; maps to Ollama queue |
| POST | `/v1/embeddings` | `openai_embeddings_handlers.rs` | Proxies to Ollama `/api/embed`; SSRF-validated |
| GET | `/v1/models` | `openai_models_handlers.rs` | Lists all Ollama + Gemini models from DB |
| GET | `/v1/models/{model_id}` | `openai_models_handlers.rs` | Looks up by model ID; 404 if not found |
| POST | `/v1/audio/transcriptions` | `openai_media_handlers.rs` | 501 Not Implemented stub |
| POST | `/v1/audio/speech` | `openai_media_handlers.rs` | 501 Not Implemented stub |
| POST | `/v1/images/generations` | `openai_media_handlers.rs` | 501 Not Implemented stub |
| POST | `/v1/moderations` | `openai_media_handlers.rs` | 501 Not Implemented stub |

All inference endpoints require `X-API-Key` auth + rate limiting.

---

## POST /v1/chat/completions

**Auth**: `X-API-Key` header (also accepts `Authorization: Bearer` and `x-goog-api-key`).
Supports both streaming (`stream: true`) and non-streaming responses.

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
    pub max_completion_tokens: Option<u32>,     // OpenAI v2 alias for max_tokens
    pub stream: Option<bool>,                   // true = SSE, false = JSON (default: false)
    pub stream_options: Option<StreamOptions>,  // { include_usage: bool }
    pub stop: Option<serde_json::Value>,        // stop sequences
    pub seed: Option<u32>,                      // reproducible outputs
    pub response_format: Option<serde_json::Value>, // json_object / json_schema / text
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub images: Option<Vec<String>>,            // base64 images for vision models
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
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","service_tier":"default","system_fingerprint":"fp_veronex","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
```

Tool calls (Ollama function calling):
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","service_tier":"default","system_fingerprint":"fp_veronex","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_0","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Seoul\"}"}}]},"finish_reason":null}]}
```

Stop/finish (with `stream_options.include_usage = true`):
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","service_tier":"default","system_fingerprint":"fp_veronex","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":10,"total_tokens":15}}

data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","service_tier":"default","system_fingerprint":"fp_veronex","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":10,"total_tokens":15}}

data: [DONE]
```

`finish_reason` is `"tool_calls"` when the model returned tool calls, `"stop"` otherwise.

**SSE chunk fields** (present on every chunk):

| Field | Value | Notes |
|-------|-------|-------|
| `service_tier` | `"default"` | Always `"default"` (OpenAI compat) |
| `system_fingerprint` | `"fp_veronex"` | Constant identifier for this server |
| `usage` | `{prompt_tokens, completion_tokens, total_tokens}` | Only present when `stream_options.include_usage = true` — emitted on the finish chunk and a final usage-only chunk (empty `choices`) |

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

---

## POST /v1/completions

Legacy text completion endpoint. Maps a single prompt to the Veronex inference queue via Ollama.

**Request fields**: `model`, `prompt` (string or array), `max_tokens`, `temperature`, `top_p`, `stream`, `stop`, `seed`, `frequency_penalty`, `presence_penalty`, `provider_type`.

**Non-streaming response**:
```json
{
  "id": "cmpl-<uuid>",
  "object": "text_completion",
  "created": 1712345678,
  "model": "llama3.2",
  "system_fingerprint": "fp_veronex",
  "choices": [{"text": "Hello!", "index": 0, "logprobs": null, "finish_reason": "stop"}],
  "usage": {"prompt_tokens": 5, "completion_tokens": 10, "total_tokens": 15}
}
```

**Streaming**: SSE with `object: "text_completion"` chunks + `[DONE]`.

---

## POST /v1/embeddings

Generates embeddings using the first available Ollama provider's `/api/embed` endpoint.

**Security**: Provider URL is SSRF-validated before each outbound request.

**Request fields**: `model` (required), `input` (string or array of strings), `encoding_format`, `user`.

**Response**:
```json
{
  "object": "list",
  "data": [{"object": "embedding", "embedding": [0.1, 0.2, ...], "index": 0}],
  "model": "nomic-embed-text",
  "usage": {"prompt_tokens": 5, "total_tokens": 5}
}
```

**Known limitation**: Picks the first available Ollama provider rather than using VRAM-aware scheduler dispatch. Does not route through Gemini.

---

## GET /v1/models

Lists all available models (Ollama models from DB + Gemini models).

**Response**:
```json
{"object": "list", "data": [{"id": "llama3.2", "object": "model", "created": 1712345678, "owned_by": "ollama"}]}
```

`owned_by` is `"ollama"` for Ollama models and `"google"` for Gemini models.

## GET /v1/models/{model_id}

Returns a single model object. Returns `404` (AppError::NotFound) with OpenAI error shape if the model does not exist.

---

## Not Implemented Stubs (501)

The following endpoints exist for protocol compatibility (Open WebUI, LiteLLM, etc.) but are not implemented. All return HTTP 501 with:
```json
{"error": {"message": "<feature> is not supported by this server", "type": "invalid_request_error", "code": "unsupported_feature"}}
```

- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `POST /v1/images/generations`
- `POST /v1/moderations`

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

## Shared Constants and Types

| Name | Location | Value/Purpose |
|------|----------|---------------|
| `SYSTEM_FINGERPRINT` | `openai_sse_types.rs` | `"fp_veronex"` — used on all response objects |
| `StreamOptions` | `openai_sse_types.rs` | `{ include_usage: Option<bool> }` — stream_options field |
| `CompletionChunk` | `openai_sse_types.rs` | SSE streaming chunk (chat.completion.chunk) |
| `ChatCompletion` | `openai_sse_types.rs` | Non-streaming response (chat.completion) |

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
