# Web — API Test & API Docs Pages

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new backend option to test page | `web/app/api-test/page.tsx` backend options + `web/messages/en.json` `apiTest.*` | Add option value + label + model fetch logic + optional filter |
| Change gemini-free filter logic | `web/app/api-test/page.tsx` `availableModels` useMemo | Modify `policyMap` filter — currently uses `available_on_free_tier` from `GeminiRateLimitPolicy` |
| Change SSE chunk parsing | `web/app/api-test/page.tsx` SSE parsing block | Modify `line.slice(6)` / `JSON.parse` logic — see SSE Parsing Rules below |
| Add new API doc link | `web/app/api-docs/page.tsx` + `web/messages/en.json` `apiDocs.*` | Add card + i18n keys in all 3 locales |
| Update OpenAPI spec | `crates/inferq/src/infrastructure/inbound/http/openapi.json` | Embedded via `include_str!` — edit JSON directly, rebuild Rust binary |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/api-test/page.tsx` | Browser SSE inference test page |
| `web/app/api-docs/page.tsx` | Links to Swagger UI, ReDoc, raw spec |
| `web/lib/api.ts` | `api.backends()`, `api.ollamaModels()`, `api.geminiModels()`, `api.geminiPolicies()` |
| `web/messages/en.json` | i18n keys under `apiTest.*`, `apiDocs.*` |

---

## /api-test — Inference Test Page

Live SSE streaming test directly in the browser.

### Backend Selection

Backend options derived from registered active backends:

| Option | Routing | Model Source |
|--------|---------|-------------|
| Ollama | `backend: "ollama"` | `GET /v1/ollama/models` — global pool (DB, distinct+sorted) |
| Gemini Free | `backend: "gemini-free"` | Global pool **filtered** by `available_on_free_tier=true` |
| Gemini | `backend: "gemini"` | Full global pool (no filter) |

- Ollama model list: `api.ollamaModels()` — **global pool** from `ollama_models` table (not per-backend)
  - No sync button on api-test page; sync from `/backends?s=ollama` → OllamaSyncSection
  - Empty state: "No models synced. Go to Backends → Ollama to sync." (`test.ollamaTestNoModels`)
  - `staleTime: 30_000` — refreshes at most every 30s
- Gemini model list: `api.geminiModels()` + `api.geminiPolicies()` fetched together when `isGeminiBackend`
- `gemini-free` filter: only models with **explicit** policy where `available_on_free_tier=true` (excluding `*`)
- `"*"` global policy is for rate limits only — not used for free-tier visibility
- Models without explicit policy → hidden from `gemini-free` list (conservative default)

### SSE Parsing Rules (api-test/page.tsx)

```typescript
// Strip ONE leading space after "data:" — preserve internal whitespace
const text = line.startsWith('data: ') ? line.slice(6) : line.slice(5)
// NOT trimStart() — that would strip meaningful leading spaces in tokens
if (text === '[DONE]') { /* stream complete */ return }
const chunk = JSON.parse(text)
const content = chunk.choices?.[0]?.delta?.content ?? ''
```

- Output rendered with `whitespace-pre-wrap` — preserves newlines and spaces
- Streaming cursor shown while active

---

## /api-docs — API Documentation Page

Links to all three documentation UIs served by the Rust API:

```
GET /docs/openapi.json   OpenAPI 3.0.3 spec (download / raw JSON)
GET /docs/swagger        Swagger UI 5 (interactive try-it-out)
GET /docs/redoc          ReDoc latest (three-panel layout)
```

Handler: `crates/inferq/src/infrastructure/inbound/http/docs_handlers.rs`
Spec file: `crates/inferq/src/infrastructure/inbound/http/openapi.json` (embedded via `include_str!`)

No authentication required for any `/docs/*` route.

---

## i18n Keys (messages/en.json)

### apiTest.*
```json
"title", "backend", "model", "prompt", "send", "clear",
"streaming", "done", "error", "selectBackend", "selectModel",
"syncModels", "noBackends", "noModels"
```

### apiDocs.*
```json
"title", "swagger", "swaggerDesc", "redoc", "redocDesc",
"openapi", "openapiDesc", "viewDocs"
```
