# Web — API Test Panel

> SSOT | **Last Updated**: 2026-03-01 (rev: single JWT-only mode; sequential numbered run tabs; no localStorage reconnect)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new backend option to test panel | `web/components/api-test-panel.tsx` backend options + `web/messages/en.json` `test.*` | Add option value + label |
| Change SSE chunk parsing | `web/components/api-test-panel.tsx` `consumeStream()` SSE parsing block | Modify `line.slice(6)` / `JSON.parse` logic |
| Add new API doc link | `web/app/api-docs/page.tsx` + `web/messages/en.json` `apiDocs.*` | Add card + i18n keys in all 3 locales |
| Update OpenAPI spec | `crates/inferq/src/infrastructure/inbound/http/openapi.json` | Embedded via `include_str!` — edit JSON directly, rebuild Rust binary |
| Change max concurrent runs | `web/components/api-test-panel.tsx` `MAX_RUNS` constant | Default: 10 (oldest run auto-removed) |

## Key Files

| File | Purpose |
|------|---------|
| `web/components/api-test-panel.tsx` | Multi-run SSE test panel component |
| `web/app/jobs/page.tsx` | Embeds `<ApiTestPanel>` above job sections |
| `web/lib/api.ts` | `api.backends()`, `api.ollamaModels()`, `api.geminiModels()`, `api.geminiPolicies()` |
| `web/messages/en.json` | i18n keys under `test.*`, `apiDocs.*` |

---

## ApiTestPanel Component

Embedded in `/jobs` page above the job sections. JWT-only — always uses the logged-in account's token.

### Layout

```
┌─ Test Panel ───────────────────────────────────────────────────────────┐
│ [Backend ▼]  [Model ▼]                                                  │
│ ┌─────────────────────────────────────────────────────────────────┐    │
│ │ Prompt...                                               [▶ Run]  │    │
│ └─────────────────────────────────────────────────────────────────┘    │
│ Running as: admin                                                        │
├────────────────────────────────────────────────────────────────────────┤
│ [#1 ✓] [#2 ⟳] [#3 ✗]                                                  │
│ ┌──────────────────────────────────────────────────────────────────┐   │
│ │ (response output for selected run tab)                            │   │
│ └──────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────┘
```

- Input area (backend selector + model selector + prompt textarea + Run button) at **top**
- Results area (tab strip + output) at **bottom**, separated by a divider
- No mode switcher, no API key input, no manual "+" tab button

### Run State Model

```tsx
interface Run {
  id: number          // sequential: 1, 2, 3…
  prompt: string      // snapshot at time of submission
  model: string
  backend: string
  status: 'idle' | 'streaming' | 'done' | 'error'
  tokens: string[]
  errorMsg: string
}
```

### Run Lifecycle

Each click of **Run**:
1. Creates a new `Run` with the next sequential id (`nextIdRef++`)
2. Appends to `runs` via `dispatch({ type: 'ADD', run })`
3. Sets `activeRunId` to the new run's id
4. Streams into that run's slot via `consumeStream()` (updates via functional `setRuns` / `dispatch`)
5. Reader stored in `readersRef: Map<runId, ReadableStreamDefaultReader>` for per-run cancellation

Tab strip behaviors:
- Tab dot colors: streaming = info, done = success, error = destructive
- Clicking a tab → `setActiveRunId(id)`
- Close (×) button → cancels reader if streaming, dispatches `REMOVE`
- Max 10 runs: oldest auto-removed when limit exceeded

### Auth & Endpoint

| Auth | Endpoint | Source | account_id |
|------|----------|--------|-----------|
| `Authorization: Bearer {JWT}` | `POST /v1/test/completions` | `test` | `claims.sub` |

- No API key required
- Jobs tracked with `account_id = claims.sub`, `api_key_id = NULL`
- Excluded from API usage/performance metrics (dashboard job counts filter `source != 'test'`)

### Backend Selection

| Option | `backend` sent | Model Source |
|--------|---------------|-------------|
| Ollama | `"ollama"` | `GET /v1/ollama/models` — global pool (DB) |
| Gemini Free | `"gemini-free"` | Global pool filtered by `available_on_free_tier=true` |
| Gemini | `"gemini"` | Full global pool (no filter) |

- `gemini-free` filter: only models with explicit policy where `available_on_free_tier=true` (excluding `*`)
- `"*"` global policy is for rate limits only — not shown in free-tier list

---

## Test Run Endpoint

`POST /v1/test/completions` — JWT Bearer authentication:
- `api_key_id = NULL`, `account_id = claims.sub`, `source = 'test'`
- No rate limiting applied
- Places job in low-priority `veronex:queue:jobs:test` queue

`GET /v1/test/jobs/{id}/stream` — JWT Bearer SSE for reconnecting to in-progress streams

---

## SSE Parsing Rules (`consumeStream()`)

```typescript
// Strip ONE leading space after "data:" — preserve internal whitespace
const text = line.startsWith('data: ') ? line.slice(6) : line.slice(5)
// NOT trimStart() — that would strip meaningful leading spaces in tokens
if (text === '[DONE]') { /* stream complete */ return }
const chunk = JSON.parse(text)

const content = chunk.choices?.[0]?.delta?.content ?? ''
```

- Output rendered with `whitespace-pre-wrap` — preserves newlines and spaces.
- Streaming cursor (blinking `▊`) shown while `status === 'streaming'`.

---

## /api-docs — API Documentation Page

Landing page at `/api-docs` links to two embedded viewers (internal Next.js routes):

| Route | Component | Notes |
|-------|-----------|-------|
| `/api-docs/swagger` | `SwaggerUiWrapper` (swagger-ui-react) | dynamic, ssr:false |
| `/api-docs/redoc`   | `RedocWrapper` (redoc)               | dynamic, ssr:false |

Both viewers auto-select the locale-aware spec URL:
```
${API_URL}/docs/openapi.json?lang={i18n.language}
```

### Locale-Aware OpenAPI Spec

```
GET /docs/openapi.json           → English (default)
GET /docs/openapi.json?lang=ko   → Korean (overlay merged)
GET /docs/openapi.json?lang=ja   → Japanese (overlay merged)
```

**Overlay files** (only translatable fields — info.description, tags, paths summaries):
```
crates/inferq/src/infrastructure/inbound/http/
  openapi.json              ← base spec (English, authoritative)
  openapi.overlay.ko.json   ← Korean translations
  openapi.overlay.ja.json   ← Japanese translations
```

Merge strategy: recursive deep merge — objects merge key-by-key, arrays/scalars replaced by overlay.

Handler: `crates/inferq/src/infrastructure/inbound/http/docs_handlers.rs`
No authentication required for any `/docs/*` route.

### ReDoc i18n (labels)

ReDoc UI chrome labels come from i18n keys under `apiDocs.redoc*`:
`redocEnum`, `redocDefault`, `redocExample`, `redocDownload`, `redocNoResults`,
`redocResponses`, `redocRequestSamples`, `redocResponseSamples`

### Key Files

| File | Purpose |
|------|---------|
| `web/components/swagger-ui-wrapper.tsx` | Swagger UI React wrapper (CSS import + theme overrides) |
| `web/components/redoc-wrapper.tsx` | RedocStandalone wrapper (Veronex theme + labels) |
| `web/app/api-docs/page.tsx` | Landing page (internal links) |
| `web/app/api-docs/swagger/page.tsx` | Swagger embedded page |
| `web/app/api-docs/redoc/page.tsx` | ReDoc embedded page |

---

## i18n Keys (messages/en.json)

### test.*
```json
"title", "backend", "model", "prompt", "send", "run", "stop", "reset", "runAgain",
"streaming", "done", "error", "output", "complete", "errorTitle",
"selectBackend", "selectModel", "noModels", "ollamaTestNoModels",
"runningAs"
```

### apiDocs.*
```json
"title", "swagger", "swaggerDesc", "redoc", "redocDesc",
"openapi", "openapiDesc", "viewDocs"
```
