# OpenAI Compat: Native Endpoints & Shared Types

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> Native inference endpoints, API doc endpoints, shared constants/types, SSE parsing, and client examples.
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
