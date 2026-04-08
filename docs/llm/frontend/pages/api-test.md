# Web — API Test Panel

> SSOT | **Last Updated**: 2026-04-06 (rev: conversation mode, context warnings, TurnInternals)
> API Docs page: `frontend/pages/api-docs.md`

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
| `web/components/api-test-panel.tsx` | Multi-run panel: state, SSE consumer, image handlers, run logic |
| `web/components/api-test-form.tsx` | Form UI: endpoint selector, API key toggle, provider/model pickers |
| `web/components/api-test-runs.tsx` | Run tabs and response output display |
| `web/components/api-test-types.ts` | Types (`Run`, `OpenAIChunk`, `Endpoint`, `RunAction`) + `runsReducer` |
| `web/app/jobs/page.tsx` | Embeds `<ApiTestPanel>` above job sections |
| `web/lib/api.ts` | `providers()`, `ollamaModels()`, `geminiModels()`, `geminiPolicies()` |
| `web/messages/en.json` | i18n keys under `test.*`, `apiDocs.*` |

## Routing

No standalone `/api-test` route. `ApiTestPanel` is embedded in `/jobs`. Old `/api-test` links redirect to `/jobs`.

## ApiTestPanel Component

Embedded in `/jobs`. Split into 3 files: `api-test-panel.tsx` (state/logic), `api-test-form.tsx` (form UI), `api-test-runs.tsx` (output display).

```
[Provider v] [Model v]
[Endpoint v: /v1/chat/completions | /api/chat | /api/generate]
[API Key: OFF/ON] [sk-... input when ON]
[Prompt...                                    [img] [Run]]
Running as: admin
[#1 ok] [#2 ...] [#3 err]
(response output for selected tab)
```

### Endpoint Selector

| Endpoint | Format | Streaming |
|----------|--------|-----------|
| `/v1/chat/completions` | OpenAI (SSE) | Yes |
| `/api/chat` | Ollama chat (JSON) | No |
| `/api/generate` | Ollama generate (JSON) | No |

Non-streaming: `/api/generate` reads `json.response`, `/api/chat` reads `json.message.content`.

### API Key Toggle

| Mode | Auth | Endpoints used |
|------|------|---------------|
| OFF (default) | JWT session cookie | `/v1/test/completions`, `/v1/test/api/chat`, `/v1/test/api/generate` |
| ON | `Bearer` (OpenAI) or `X-API-Key` (Ollama) | `/v1/chat/completions`, `/api/chat`, `/api/generate` |

When ON, OpenAI endpoint uses `Authorization: Bearer {key}`, Ollama uses `X-API-Key: {key}`.

### Run State

| Field | Type | Notes |
|-------|------|-------|
| `id` | `number` | Sequential: 1, 2, 3... |
| `prompt` | `string` | Snapshot at submission |
| `model` | `string` | |
| `provider_type` | `string` | |
| `endpoint` | `Endpoint` | Selected endpoint path |
| `useApiKey` | `boolean` | API key mode at submission |
| `status` | `'idle' \| 'streaming' \| 'done' \| 'error'` | |
| `text` | `string` | Accumulated response text |
| `errorMsg` | `string` | |
| `images` | `string[] \| undefined` | Attached base64 images |

### Run Lifecycle

1. Creates `Run` with `nextIdRef++`, appends via `dispatch({ type: 'ADD', run })`
2. Sets `activeRunId` to new run
3. SSE endpoints: streams via `consumeStream()`, reader in `readersRef`
4. JSON endpoints: awaits response, extracts text, sets done

Dot colors: streaming=info, done=success, error=destructive. Close cancels reader. Max 10 runs.

### Auth and Endpoint

| Auth | Endpoint | Source | account_id |
|------|----------|--------|-----------|
| JWT session | `POST /v1/test/completions` | `test` | `claims.sub` |
| JWT session | `POST /v1/test/api/chat` | `test` | `claims.sub` |
| JWT session | `POST /v1/test/api/generate` | `test` | `claims.sub` |
| API Key | Real endpoints (3 above) | `api` or `api_paid` | key's tenant |

Test jobs: `api_key_id = NULL`, excluded from usage/perf metrics (`source != 'test'`).

### Provider Selection

| Option | `provider_type` sent | Model source |
|--------|---------------|-------------|
| Ollama | `"ollama"` | `GET /v1/ollama/models` (global pool) |
| Gemini Free | `"gemini-free"` | Filtered by `available_on_free_tier=true` |
| Gemini | `"gemini"` | Full global pool |

Gemini options visible only when `labSettings.gemini_function_calling` enabled. Gate uses `useLabSettings()`.

### Conversation Mode (Multi-Turn)

Mode toggle: `single` | `conversation`. In conversation mode:
- New conversation session created on first send
- `X-Conversation-ID` response header persisted per session as `conversationId`
- Each turn sends `X-Conversation-ID: {conversationId}` header
- If `conversation_renewed: true` → system message divider inserted: "Session renewed — context was compressed and continued in a new conversation"
- Multiple sessions supported (tab strip in `ApiTestConversation`)
- `ConversationSession` state: `{ id, conversationId?, messages, streamingText, status, errorMsg }`

### Context Window Warnings

`getMultiturnWarnings()` (`api-test-form.tsx`) shows warning badges below model selector in conversation mode:

| Warning key | Condition |
|-------------|-----------|
| `model_too_small` | Model param count < `multiturn_min_params` |
| `multiturn_warn_ctx_too_small` | Model `max_ctx` < `multiturn_min_ctx` |
| `model_not_allowed` | Model not in `multiturn_allowed_models` |
| `context_too_large` | Estimated tokens > 85% of model's `max_ctx` |

Token estimation: `sum(message.content.length / 3.5)`. `max_ctx` from `GET /v1/ollama/models`.

### TurnInternals Panel

`TurnInternals` (`web/components/turn-internals.tsx`) — collapsible per-turn panel.
- Lazy-fetches on expand: `GET /v1/dashboard/conversations/{convId}/turns/{jobId}/internals`
- Stale time: `STALE_TIME_SLOW` (59s)
- Shows `CompressedTurn`: compression model, original/compressed tokens, ratio, summary
- Shows `VisionAnalysis`: vision model, image count, analysis tokens, analysis text

## Test Endpoints

| Method | Path | Auth | Notes |
|--------|------|------|-------|
| POST | `/v1/test/completions` | JWT | OpenAI format, `source='test'`, no rate limiting |
| POST | `/v1/test/api/chat` | JWT | Ollama chat format, `source='test'` |
| POST | `/v1/test/api/generate` | JWT | Ollama generate format, `source='test'` |
| GET | `/v1/test/jobs/{id}/stream` | JWT | SSE reconnect for in-progress streams |

## SSE Parsing (`consumeStream()`)

Strip one leading space after `data:`. `[DONE]` = stream complete. Parse `chunk.choices?.[0]?.delta?.content ?? ''`. Output `whitespace-pre-wrap`. Blinking cursor while `status === 'streaming'`.

## i18n Keys

`test.*`: title, provider, model, prompt, send, run, stop, reset, runAgain, streaming, done, error, output, complete, errorTitle, selectProvider, selectModel, noModels, ollamaTestNoModels, runningAs, endpoint, apiKeyToggle, noApiKey, apiKeyPlaceholder, imageAttach, imageRemove, imageCompressing
