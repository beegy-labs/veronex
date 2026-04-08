# OpenAI-Compatible API — Secondary Endpoints

> SSOT | **Last Updated**: 2026-03-15
> Primary endpoint (POST /v1/chat/completions): `inference/openai-compat.md`

---

## POST /v1/completions

Legacy text completion. Maps a single prompt to the Veronex inference queue via Ollama.

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

Generates embeddings using the first available Ollama provider's `/api/embed`.

**Security**: Provider URL SSRF-validated before each outbound request.

**Request fields**: `model` (required), `input` (string or array), `encoding_format`, `user`.

**Response**:
```json
{
  "object": "list",
  "data": [{"object": "embedding", "embedding": [0.1, 0.2], "index": 0}],
  "model": "nomic-embed-text",
  "usage": {"prompt_tokens": 5, "total_tokens": 5}
}
```

**Known limitation**: Picks first available Ollama provider — not VRAM-aware. Does not route through Gemini.

---

## GET /v1/models

Lists all available models (Ollama from DB + Gemini).

**Response**:
```json
{"object": "list", "data": [{"id": "llama3.2", "object": "model", "created": 1712345678, "owned_by": "ollama"}]}
```

`owned_by`: `"ollama"` for Ollama models, `"google"` for Gemini models.

## GET /v1/models/{model_id}

Returns a single model object. Returns `404` (AppError::NotFound) with OpenAI error shape if not found.

---

## Not Implemented Stubs (501)

Endpoints for protocol compatibility (Open WebUI, LiteLLM, etc.), not implemented. All return HTTP 501 with:
```json
{"error": {"message": "<feature> is not supported by this server", "type": "invalid_request_error", "code": "unsupported_feature"}}
```

- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `POST /v1/images/generations`
- `POST /v1/moderations`
