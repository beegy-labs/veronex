# OpenAI-Compatible Inference API

> SSOT | **Last Updated**: 2026-03-15
> Secondary endpoints (completions, embeddings, models, stubs): `inference/openai-compat-endpoints.md`

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new `provider_type` value | `openai_handlers.rs` `chat_completions()` + `provider_router.rs` dispatch | Add new arm to `provider_type` match |
| Change SSE chunk format | `openai_sse_types.rs` | Modify `CompletionChunk` / `DeltaContent` structs |
| Add new field to ChatCompletionRequest | `openai_handlers.rs` `ChatCompletionRequest` struct | Add field + propagate to `submit()` |
| Update OpenAPI spec | `infrastructure/inbound/http/openapi.json` | Edit JSON directly — embedded via `include_str!` |
| Change /docs auth | `router.rs` | Move `/docs/*` routes inside auth middleware layer |
| Add a new 501 stub endpoint | `openai_media_handlers.rs` | Add async fn + register in `router.rs` |

## Key Files

| File | Purpose |
|------|---------|
| `infrastructure/inbound/http/openai_handlers.rs` | `chat_completions` handler + Ollama proxy + legacy queue path |
| `infrastructure/inbound/http/openai_sse_types.rs` | Shared SSE/response types: `CompletionChunk`, `DeltaContent`, `ChatCompletion` |
| `infrastructure/inbound/http/openai_completions_handlers.rs` | `POST /v1/completions` — legacy text completions |
| `infrastructure/inbound/http/openai_embeddings_handlers.rs` | `POST /v1/embeddings` — proxies to Ollama /api/embed |
| `infrastructure/inbound/http/openai_models_handlers.rs` | `GET /v1/models`, `GET /v1/models/{model_id}` |
| `infrastructure/inbound/http/openai_media_handlers.rs` | `POST /v1/audio/*`, `/v1/images/generations`, `/v1/moderations` — 501 stubs |
| `infrastructure/inbound/http/docs_handlers.rs` | Swagger / ReDoc / OpenAPI spec |
| `infrastructure/inbound/http/openapi.json` | OpenAPI 3.0.3 spec (embedded via `include_str!`) |

---

## Endpoint Summary

| Method | Path | Handler file | Notes |
|--------|------|--------------|-------|
| POST | `/v1/chat/completions` | `openai_handlers.rs` | Streaming + non-streaming; Ollama proxy + Gemini legacy path |
| POST | `/v1/completions` | `openai_completions_handlers.rs` | Legacy text completion |
| POST | `/v1/embeddings` | `openai_embeddings_handlers.rs` | Proxies to Ollama `/api/embed` |
| GET | `/v1/models` | `openai_models_handlers.rs` | Lists all Ollama + Gemini models from DB |
| GET | `/v1/models/{model_id}` | `openai_models_handlers.rs` | 404 if not found |
| POST | `/v1/audio/transcriptions` | `openai_media_handlers.rs` | 501 stub |
| POST | `/v1/audio/speech` | `openai_media_handlers.rs` | 501 stub |
| POST | `/v1/images/generations` | `openai_media_handlers.rs` | 501 stub |
| POST | `/v1/moderations` | `openai_media_handlers.rs` | 501 stub |

All inference endpoints require `X-API-Key` auth + rate limiting.

---

## POST /v1/chat/completions

**Auth**: `X-API-Key` (also accepts `Authorization: Bearer` and `x-goog-api-key`).
Supports streaming (`stream: true`) and non-streaming.

### Request Struct

```rust
// openai_handlers.rs
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub provider_type: Option<String>,          // "ollama" | "gemini-free" | "gemini"
    pub tools: Option<Vec<serde_json::Value>>,
    pub tool_choice: Option<serde_json::Value>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<u32>,                // maps to Ollama options.num_predict
    pub max_completion_tokens: Option<u32>,     // OpenAI v2 alias
    pub stream: Option<bool>,
    pub stream_options: Option<StreamOptions>,  // { include_usage: bool }
    pub stop: Option<serde_json::Value>,
    pub seed: Option<u32>,
    pub response_format: Option<serde_json::Value>,
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub images: Option<Vec<String>>,            // base64 images for vision
}

pub struct ChatMessage {
    pub role: String,                           // "system" | "user" | "assistant" | "tool"
    pub content: Option<MessageContent>,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}
```

### Two Execution Paths

| `provider_type` | Path | Behavior |
|-----------------|------|----------|
| `"ollama"` (default) | **Ollama proxy** | Full conversation history forwarded. Tools, temperature, top_p, max_tokens passed through. Tool call args: OpenAI JSON string → Ollama JSON object. |
| `"gemini-free"`, `"gemini"` | **Legacy queue** | Only last `user` message extracted as prompt. Enqueued via Valkey queue. |

### `provider_type` Field

| Value | Routing |
|-------|---------|
| `"ollama"` | VRAM-aware Ollama selection |
| `"gemini-free"` | `is_free_tier=true` only |
| `"gemini"` | Free-first → paid fallback on RPD exhaustion |

`"gemini-free"` maps to `ProviderType::Gemini` with `tier_filter = Some("free")`. `ProviderType` enum has only two variants: `Ollama` and `Gemini`.

### SSE Response

Content token:
```
data: {"id":"chatcmpl-…","object":"chat.completion.chunk","model":"llama3.2","service_tier":"default","system_fingerprint":"fp_veronex","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
```

Stop/finish (with `stream_options.include_usage = true`):
```
data: {"id":"chatcmpl-…","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":10,"total_tokens":15}}

data: [DONE]
```

| Field | Value | Notes |
|-------|-------|-------|
| `service_tier` | `"default"` | Always |
| `system_fingerprint` | `"fp_veronex"` | Constant |
| `usage` | `{prompt_tokens, completion_tokens, total_tokens}` | Only with `stream_options.include_usage=true` |

`finish_reason`: `"tool_calls"` when model returned tool calls, `"stop"` otherwise.

### Non-Streaming — Conversation Tracking

When `X-Conversation-ID` header present, additional fields in `ChatCompletion` response:

| Field | Type | Notes |
|-------|------|-------|
| `conversation_id` | `string` | Active conversation UUID |
| `conversation_renewed` | `bool` | `true` when handoff created a new conversation |

`X-Conversation-ID` response header always set when conversation context is active.

→ See `inference/context-compression.md` for full multi-turn / handoff flow.

### Error Response

```json
{ "error": { "message": "failed to submit inference job" } }
```

**429**: Returned with `Retry-After: 60` when all Gemini providers are rate-limited.

### Input Validation

| Field | Limit | Constant |
|-------|-------|----------|
| Model name | max 256 bytes | `MAX_MODEL_NAME_BYTES` |
| Total message content | max 1MB | `MAX_PROMPT_BYTES` |

---

→ Native endpoints, shared types, SSE parsing: `openai-compat-native.md`
