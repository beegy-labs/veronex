# Web — API Test Panel

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new backend option to test panel | `web/components/api-test-panel.tsx` backend options + `web/messages/en.json` `test.*` | Add option value + label |
| Change SSE chunk parsing | `web/components/api-test-panel.tsx` `consumeStream()` SSE parsing block | Modify `line.slice(6)` / `JSON.parse` logic |
| Add new API doc link | `web/app/api-docs/page.tsx` + `web/messages/en.json` `apiDocs.*` | Add card + i18n keys in all 3 locales |
| Update OpenAPI spec | `crates/inferq/src/infrastructure/inbound/http/openapi.json` | Embedded via `include_str!` — edit JSON directly, rebuild Rust binary |
| Change localStorage key format | `web/components/api-test-panel.tsx` `lsKey` constant | Format: `veronex:test:tab:{tabKey}` |

## Key Files

| File | Purpose |
|------|---------|
| `web/components/api-test-panel.tsx` | Multi-tab SSE test panel component |
| `web/app/jobs/page.tsx` | Embeds `<ApiTestPanel>` + handles retry |
| `web/lib/api.ts` | `api.backends()`, `api.ollamaModels()`, `api.geminiModels()`, `api.geminiPolicies()` |
| `web/messages/en.json` | i18n keys under `test.*`, `apiDocs.*` |

---

## ApiTestPanel Component

Embedded in `/jobs` page above the job sections. Manages multiple test tabs.

### Multi-Tab Structure

```
┌─ Test Panel ───────────────────────────────────────────────────────────┐
│ [Tab 1] [Tab 2] [+]                                                     │
├────────────────────────────────────────────────────────────────────────┤
│ Backend: [ollama ▼]   Model: [llama3.2 ▼]                             │
│ Prompt: _______________________________________________                 │
│ [Send]  [Clear]                                                         │
│                                                                         │
│ ┌─ Output ──────────────────────────────────────────────────────────┐  │
│ │ Streaming tokens appear here...▊                                   │  │
│ └────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

- Each tab is a `TestSession` component with its own state.
- Tab IDs are integers starting at 1, assigned on creation (`nextId` ref).
- Each tab gets a unique `tabKey` prop (= `tab.id`) for localStorage isolation.

### Backend Selection

| Option | `backend` sent | Model Source |
|--------|---------------|-------------|
| Ollama | `"ollama"` | `GET /v1/ollama/models` — global pool (DB) |
| Gemini Free | `"gemini-free"` | Global pool filtered by `available_on_free_tier=true` |
| Gemini | `"gemini"` | Full global pool (no filter) |

- `gemini-free` filter: only models with explicit policy where `available_on_free_tier=true` (excluding `*`)
- `"*"` global policy is for rate limits only — not shown in free-tier list

---

## source=test Tagging

All test panel submissions send `"source": "test"` in the request body:

```typescript
// api-test-panel.tsx — doSubmit()
body: JSON.stringify({
  model,
  messages: [{ role: 'user', content: prompt }],
  stream: true,
  source: 'test',    // ← routes to veronex:queue:jobs:test
})
```

This places the job into the low-priority test queue (polled after the API queue by BLPOP).

---

## localStorage Reconnect (Test Sessions)

When a test job starts, its `job_id` is persisted in `localStorage` so the stream can be recovered if the user navigates away.

### Key Format

```
veronex:test:tab:{tabKey}
```

Where `tabKey` = `tab.id` (integer, starts at 1 per session). On page reload, the first tab gets ID 1 and will find `veronex:test:tab:1` from the previous session.

### Lifecycle

```
doSubmit() receives first SSE chunk with chunk.id (job_id)
  → localStorage.setItem(lsKey, JSON.stringify({ jobId: chunk.id }))

Page unmount / refresh / navigate away...

TestSession mounts (useEffect):
  → localStorage.getItem(lsKey)
  → if jobId found → reconnectStream(jobId)

reconnectStream(savedJobId):
  → GET /v1/jobs/{savedJobId}/stream  (auth via NEXT_PUBLIC_VERONEX_ADMIN_KEY)
  → consumes OpenAI SSE format: data: {choices...} → data: [DONE]
  → re-uses consumeStream() helper (same parsing as live stream)

handleReset():
  → localStorage.removeItem(lsKey)
  → clears output + status
```

### Reconnect Endpoint

```
GET /v1/jobs/{id}/stream
    Header: X-API-Key: <admin key>
    → OpenAI-format SSE
    → For completed jobs: replays result_text as single chunk + [DONE]
    → For in-progress jobs: attaches to live token stream
```

---

## SSE Parsing Rules (`consumeStream()`)

```typescript
// Strip ONE leading space after "data:" — preserve internal whitespace
const text = line.startsWith('data: ') ? line.slice(6) : line.slice(5)
// NOT trimStart() — that would strip meaningful leading spaces in tokens
if (text === '[DONE]') { /* stream complete */ return }
const chunk = JSON.parse(text)

// Save job_id for reconnect (first chunk only)
if (!jobId && chunk.id) {
  jobId = chunk.id
  localStorage.setItem(lsKey, JSON.stringify({ jobId }))
}

const content = chunk.choices?.[0]?.delta?.content ?? ''
```

- Output rendered with `whitespace-pre-wrap` — preserves newlines and spaces.
- Streaming cursor (blinking `▊`) shown while `status === 'streaming'`.
- `consumeStream()` is shared between `doSubmit()` (live) and `reconnectStream()` (replay).

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
"title", "backend", "model", "prompt", "send", "clear",
"streaming", "done", "error", "selectBackend", "selectModel",
"noModels", "ollamaTestNoModels"
```

### apiDocs.*
```json
"title", "swagger", "swaggerDesc", "redoc", "redocDesc",
"openapi", "openapiDesc", "viewDocs"
```
