# Web — API Test Panel

> SSOT | Last Updated: 2026-03-04 (rev: JWT-only; sequential run tabs; no localStorage reconnect)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add provider option | `api-test-panel.tsx` provider options + `en.json` `test.*` | Add option value + label |
| Change SSE chunk parsing | `api-test-panel.tsx` `consumeStream()` | Modify `line.slice(6)` / `JSON.parse` logic |
| Add API doc link | `web/app/api-docs/page.tsx` + `en.json` `apiDocs.*` | Add card + i18n keys |
| Update OpenAPI spec | `openapi.json` in `infrastructure/inbound/http/` | Edit JSON directly, rebuild binary |
| Change max concurrent runs | `api-test-panel.tsx` `MAX_RUNS` | Default: 10 (oldest auto-removed) |

## Key Files

| File | Purpose |
|------|---------|
| `web/components/api-test-panel.tsx` | Multi-run SSE test panel |
| `web/app/jobs/page.tsx` | Embeds `<ApiTestPanel>` above job sections |
| `web/lib/api.ts` | `providers()`, `ollamaModels()`, `geminiModels()`, `geminiPolicies()` |
| `web/messages/en.json` | i18n keys under `test.*`, `apiDocs.*` |

## Routing

There is no standalone `/api-test` route. The `ApiTestPanel` component is embedded directly in the `/jobs` page. Any old `/api-test` links should redirect to `/jobs`.

## ApiTestPanel Component

Embedded in `/jobs` page. JWT-only -- uses logged-in account's token.

```
[Provider v] [Model v]
[Prompt...                                         [Run]]
Running as: admin
[#1 ok] [#2 ...] [#3 err]
(response output for selected tab)
```

No mode switcher, no API key input, no manual "+" tab button.

### Run State

| Field | Type | Notes |
|-------|------|-------|
| `id` | `number` | Sequential: 1, 2, 3... |
| `prompt` | `string` | Snapshot at submission |
| `model` | `string` | |
| `provider_type` | `string` | |
| `status` | `'idle' \| 'streaming' \| 'done' \| 'error'` | |
| `tokens` | `string[]` | Accumulated SSE chunks |
| `errorMsg` | `string` | |

### Run Lifecycle

1. Creates new `Run` with `nextIdRef++`, appends via `dispatch({ type: 'ADD', run })`
2. Sets `activeRunId` to new run, streams via `consumeStream()`
3. Reader stored in `readersRef: Map<runId, Reader>` for per-run cancellation

Tab behaviors: dot colors (streaming=info, done=success, error=destructive). Close cancels reader if streaming, dispatches `REMOVE`. Max 10 runs.

### Auth and Endpoint

| Auth | Endpoint | Source | account_id |
|------|----------|--------|-----------|
| `Bearer {JWT}` | `POST /v1/test/completions` | `test` | `claims.sub` |

No API key required. Jobs tracked with `api_key_id = NULL`. Excluded from usage/perf metrics (`source != 'test'`).

### Provider Selection

| Option | `provider_type` sent | Model source |
|--------|---------------|-------------|
| Ollama | `"ollama"` | `GET /v1/ollama/models` (global pool) |
| Gemini Free | `"gemini-free"` | Filtered by `available_on_free_tier=true` |
| Gemini | `"gemini"` | Full global pool |

`gemini-free`: only models with explicit policy `available_on_free_tier=true` (excluding `*`). The `*` global policy is for rate limits only.

## Test Endpoints

| Method | Path | Auth | Notes |
|--------|------|------|-------|
| POST | `/v1/test/completions` | JWT | `source='test'`, no rate limiting, low-priority queue |
| GET | `/v1/test/jobs/{id}/stream` | JWT | SSE reconnect for in-progress streams |

## SSE Parsing (`consumeStream()`)

Strip one leading space after `data:` (preserve internal whitespace). `[DONE]` = stream complete. Parse `chunk.choices?.[0]?.delta?.content ?? ''`. Output rendered with `whitespace-pre-wrap`. Blinking cursor while `status === 'streaming'`.

## /api-docs Page

Landing page links to two embedded viewers:

| Route | Component | Notes |
|-------|-----------|-------|
| `/api-docs/swagger` | `SwaggerUiWrapper` (swagger-ui-react) | dynamic, ssr:false |
| `/api-docs/redoc` | `RedocWrapper` (redoc) | dynamic, ssr:false |

Both auto-select locale-aware spec: `${API_URL}/docs/openapi.json?lang={locale}`

### Locale-Aware OpenAPI Spec

| Path | Lang |
|------|------|
| `GET /docs/openapi.json` | English (default) |
| `GET /docs/openapi.json?lang=ko` | Korean overlay |
| `GET /docs/openapi.json?lang=ja` | Japanese overlay |

Overlays in `crates/veronex/src/infrastructure/inbound/http/openapi.overlay.{ko,ja}.json`. Merge: recursive deep merge (objects merge key-by-key, arrays/scalars replaced). Handler: `docs_handlers.rs`. No auth required.

### /api-docs Key Files

| File | Purpose |
|------|---------|
| `web/components/swagger-ui-wrapper.tsx` | Swagger UI wrapper (CSS + theme) |
| `web/components/redoc-wrapper.tsx` | RedocStandalone wrapper (theme + labels) |
| `web/app/api-docs/page.tsx` | Landing page |
| `web/app/api-docs/swagger/page.tsx` | Swagger embedded |
| `web/app/api-docs/redoc/page.tsx` | ReDoc embedded |

## i18n Keys

`test.*`: title, provider, model, prompt, send, run, stop, reset, runAgain, streaming, done, error, output, complete, errorTitle, selectProvider, selectModel, noModels, ollamaTestNoModels, runningAs

`apiDocs.*`: title, swagger, swaggerDesc, redoc, redocDesc, openapi, openapiDesc, viewDocs, redocEnum, redocDefault, redocExample, redocDownload, redocNoResults, redocResponses, redocRequestSamples, redocResponseSamples
