# Web — Providers Page (/providers)

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add action button to Ollama backend row | `web/app/providers/page.tsx` `OllamaTab` row actions | Same pattern as existing actions |
| Add action button to Gemini paid backend row | `web/app/providers/page.tsx` `GeminiTab` Gemini API Keys Table | Paid vs free tier conditional (`!backend.is_free_tier`) |
| Add field to RegisterOllamaModal | `web/app/providers/page.tsx` modal form state + `web/lib/api.ts` `registerBackend()` | Add field → pass to `api.registerBackend(body)` |
| Change OllamaSyncSection empty state | `web/app/providers/page.tsx` `OllamaSyncSection` + `web/messages/en.json` `backends.ollama.ollamaNoSync` | Update i18n key in all 3 locales |
| Change table page size | `web/app/providers/page.tsx` `PAGE_SIZE` constant | Single constant used by all 3 tables |
| Change rate limit policy table columns | `web/app/providers/page.tsx` `GeminiSyncSection` table | Add/remove column header + cell render |
| Change ModelSelectionModal empty state | `web/app/providers/page.tsx` `ModelSelectionModal` + `web/messages/en.json` `backends.noGlobalModels` | Update i18n key in all 3 locales |
| Change Ollama live metrics refresh interval | `web/app/providers/page.tsx` `OllamaServerMetrics` `refetchInterval` | Default: 30 000 ms |
| Add live metric field to Ollama server cell | `web/app/providers/page.tsx` `OllamaServerMetrics` render | Add field from `NodeMetrics.gpus[n]` |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/providers/page.tsx` | All 2 tabs + all modals (OllamaTab, GeminiTab) |
| `web/lib/api.ts` | `api.servers()`, `api.backends()`, `api.ollamaModels()`, `api.syncOllamaModels()`, `api.ollamaSyncStatus()`, `api.geminiModels()`, `api.syncGeminiStatus()`, etc. |
| `web/lib/types.ts` | `LlmBackend`, `GpuServer`, `OllamaSyncJob`, `GeminiRateLimitPolicy`, `GeminiModel`, `BackendSelectedModel`, `GeminiStatusSyncResponse` |
| `web/messages/en.json` | i18n keys under `backends.*` |

---

## Routing

URL `?s=` param (default: `ollama`):

| URL | Section |
|-----|---------|
| `/providers` or `?s=ollama` | `OllamaTab` — Ollama backend management |
| `?s=gemini` | `GeminiTab` — Gemini + rate-limit policies |

Section switching via `<Link>` in `nav.tsx` — no internal tab state in page.

Nav entry: `NavGroup` with `id: 'providers'`, `basePath: '/providers'`, `Server` icon.

---

## Pagination (all three backend tables)

`PAGE_SIZE = 10` (single constant, shared).

Each table (Ollama, Gemini, Servers) uses local `page` state. Pattern:

```typescript
const [page, setPage] = useState(1)
const totalPages = Math.max(1, Math.ceil(items.length / PAGE_SIZE))
const safePage = Math.min(page, totalPages)
const pageItems = items.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE)
```

Controls rendered inside `<CardContent>` below `</Table>`, separated by `border-t border-border`:
- Range: `{start}–{end} / {total}` (text-xs)
- `ChevronLeft` / `ChevronRight` icon buttons — disabled at boundary
- Hidden when `totalPages <= 1`

---

## OllamaTab — Ollama Backends

Header shows pill badges: `N registered` · `● N online` · `N offline`.

```
[N registered] [● N online] [N offline]              [+ Register Ollama Backend]

Name/URL           Server / GPU / VRAM / Live Metrics         Status    Registered  Actions
────────────────────────────────────────────────────────────────────────────────────────────
gpu-ollama-1       ● gpu-node-1                               ● online  Feb 26      [↻][🔄][⊞][✏️][🗑]
http://host:11434    GPU 0  VRAM 32 GB
                   ─────────────────────────────
                   MEM 12.4 GB/64 GB  🌡 52°C  ⚡ 45W

                 [← 1 / 2 →]   ← pagination controls
```

Actions: [↻ Healthcheck] [🔄 Sync Models] [⊞ Models (`ListFilter`)] [✏️ Edit] [🗑 Delete]

- **[🔄 Sync Models]**: `POST /v1/backends/{id}/models/sync` — also persists to `ollama_models` table. Invalidates `['ollama-sync-status']` and `['ollama-models']`.
- **[⊞ Models]** (ListFilter icon): opens `OllamaBackendModelsModal` — shows all models synced for that backend.

### OllamaServerMetrics (inline in Server column)

Component `OllamaServerMetrics({ serverId, gpuIndex })` renders below the GPU/VRAM info for each row that has a linked server.

```typescript
useQuery<NodeMetrics>({
  queryKey: ['server-metrics', serverId],
  queryFn: () => api.serverMetrics(serverId),
  refetchInterval: 30_000,   // auto-refresh every 30 s
  retry: false,
})
```

Displays (compact single line):
- `MEM used/total` — `mem_total_mb - mem_available_mb` / `mem_total_mb`
- `🌡 N°C` — `gpus[gpuIndex ?? 0].temp_c`; colored: red ≥85°C, amber ≥70°C, grey otherwise
- `⚡ NW` — `gpus[gpuIndex ?? 0].power_w`
- If `scrape_ok === false` or error → italic `"unreachable"` in red
- Hidden when no server is linked (`server_id` is null)

**RegisterOllamaModal fields**: name, URL, total_vram_mb, gpu_index, server_id (dropdown)

**EditModal fields**: name, URL, api_key (blank = keep existing), total_vram_mb, gpu_index, server_id

### OllamaBackendModelsModal

Opened by [⊞] on a backend row. Shows models from DB (`GET /v1/ollama/backends/{id}/models`).

- `queryKey: ['ollama-backend-models', backendId]`, `staleTime: 30_000`
- Search filters badges client-side

### OllamaSyncSection — Global Model Sync

Defined as `OllamaSyncSection` component at the bottom of `OllamaTab`.

```
┌─────────────────────────────────────────────────────────────────┐
│ Global Ollama Model Sync                                        │
│ [Sync All ↻]  2/3 backends (shown while running)               │
│                                                                  │
│ [🔍 Search models…]            3 / 5                           │
│ Available models                                                 │
│ ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ │
│  🖥 codellama            [🖥 2]  ← click → OllamaModelBackends │
│  🖥 llama3               [🖥 3]                                 │
└─────────────────────────────────────────────────────────────────┘
```

**State** (inside OllamaSyncSection):
```typescript
const { data: syncJob } = useQuery({
  queryKey: ['ollama-sync-status'],
  queryFn: api.ollamaSyncStatus,
  refetchInterval: (q) => q.state.data?.status === 'running' ? 2000 : false,
  retry: false,
})
const { data: ollamaModelsData } = useQuery({
  queryKey: ['ollama-models'],
  queryFn: api.ollamaModels,         // → { models: OllamaModelWithCount[] }
  staleTime: 30_000,
})
```

- **Sync All**: `POST /v1/ollama/models/sync` → invalidates `['ollama-sync-status']` + `['ollama-models']`
- Button disabled while running
- Model list: searchable, filtered client-side, shows filtered/total count
- Each row clickable → opens `OllamaModelBackendsModal`

### OllamaModelBackendsModal

Opened by clicking a model row in OllamaSyncSection.

- `queryKey: ['ollama-model-backends', modelName]`, `staleTime: 30_000`
- `GET /v1/ollama/models/{model_name}/backends`
- **Pagination**: `PAGE_SIZE = 8`; Prev/Next buttons; page resets when search changes
- Search filters by name OR url (host portion)
- Status dot + badge: green=online, amber=degraded, red=offline

---

## GeminiTab — Three Sections

Section header shows pill badges: `N registered` · `N active` · `● N online` · `N degraded` · `N offline`.

### 1. Gemini API Keys Table

```
[N registered] [✓ N active] [● N online]             [+ Register Gemini Backend]

Name     API Key        Free Tier  Active  Status    Registered  Actions
acc-1    AIza...abc ●   [Free]     ●       ● online  Feb 26      [↻][✏️][🗑]
acc-2    AIza...xyz ●   [Paid]     ●       ● online  Feb 26      [↻][⊞][✏️][🗑]

                 [← 1 / 2 →]   ← pagination controls
```

Free tier actions: [↻ Healthcheck] [✏️ Edit] [🗑 Delete]
Paid actions (+): [⊞ Model Selection (`ListFilter` icon)]

**RegisterGeminiModal / EditModal**: name, api_key (blank on edit = keep), Free Tier toggle

> **Gemini status is NOT auto-checked** by the background health checker.
> Status is only updated via the GeminiStatusSyncSection "Sync Status" button or
> per-row [↻ Healthcheck] button.

### 2. GeminiStatusSyncSection — Manual Status Sync

Placed between the Gemini API Keys Table and GeminiSyncSection.

```
┌─────────────────────────────────────────────────────────────────┐
│ ↻ Gemini Status Sync                                            │
│ Manually check connectivity for all active Gemini backends.     │
│                                                                  │
│ [Sync Status ↻]  ✓ Status updated — 2/2 online                 │
│                                                                  │
│  ● gemini-free-1    online                                      │
│  ○ gemini-paid-1    offline                                     │
└─────────────────────────────────────────────────────────────────┘
```

- `POST /v1/gemini/sync-status` → `GeminiStatusSyncResponse { synced_at, results[{ id, name, status, error }] }`
- `onSuccess`: invalidates `['backends']`
- Result list shown after sync; empty when no backends

### 3. GeminiSyncSection — Global Model Sync + Rate Limit Policies

Defined as separate component `GeminiSyncSection` inside `GeminiTab`.

**State** (inside GeminiSyncSection):
```typescript
const [editingPolicy, setEditingPolicy] = useState<GeminiRateLimitPolicy | null>(null)

const { data: modelsData } = useQuery({ queryKey: ['gemini-models'], queryFn: api.geminiModels })
const { data: policies }   = useQuery({ queryKey: ['gemini-policies'], queryFn: api.geminiPolicies })
const { data: syncConfig } = useQuery({ queryKey: ['gemini-sync-config'], queryFn: api.geminiSyncConfig })
```

**Sync Controls Card**: Admin API Key (masked) + Edit button → `SetSyncKeyModal` + Sync Now → `POST /v1/gemini/models/sync`

**Rate Limit Table** (per-model policies only — no global `*` default row):

```
Model                Free Tier    RPM   RPD   Last Updated   Edit
gemini-2.5-pro       [Enabled]      5   100   Feb 27        ✏️
gemini-2.5-flash     [Enabled]     10   250   Feb 27        ✏️
gemini-2.5-flash-lite [global]      —    —    —             ✏️   ← opacity-60 (inherited)
```

> The `*` (global default) row is **not displayed** in the UI — it exists in DB for routing
> fallback only. Editing the global default is not exposed in the UI.

- Synced models with specific policy → shown normally
- Synced models without specific policy → `opacity-60`, `global default` label in Free Tier cell, date = `—`
- Edit → `EditPolicyModal`

**EditPolicyModal**:
```
Model: gemini-2.5-flash
[Available on Free Tier] ─── Switch
  → on:  RPM (req/min) | RPD (req/day) inputs
  → off: inputs hidden (paid-only, no counter)
[Cancel] [Save] → api.upsertGeminiPolicy(model_name, request)
```

**SetSyncKeyModal**: password input → `PUT /v1/gemini/sync-config`. Invalidates `['gemini-sync-config']`.

### 4. ModelSelectionModal (paid backends)

Opened by `ListFilter` button on paid backend rows.

- Data: `GET /v1/backends/{id}/selected-models` → global models merged with per-backend state
- Toggle → `PATCH /v1/backends/{id}/selected-models/{model_name}` `{ is_enabled: bool }`
- Optimistic update: switch flips immediately, reverts on error
- Empty state: "No global models. Set an admin key and click Sync Now."
- `useQuery({ queryKey: ['selected-models', backendId] })`

---

## i18n Keys (messages/en.json → `backends.*`)

```json
"registerServer", "editServer", "serverName", "nodeExporterUrl",
"registerOllama", "editOllama", "backendUrl", "totalVram", "gpuIndex",
"ollama.ollamaSyncSection", "ollama.ollamaSyncAll", "ollama.ollamaSyncing",
"ollama.ollamaSyncDone", "ollama.ollamaAvailableModels", "ollama.ollamaNoSync",
"ollama.ollamaSearchModels", "ollama.viewModels",
"registerGemini", "editGemini", "apiKey", "freeTier",
"gemini.statusSyncSection", "gemini.statusSyncDesc", "gemini.syncStatus",
"gemini.syncingStatus", "gemini.statusSyncDone", "gemini.noStatusResults",
"syncSection", "syncKey", "setSyncKey", "syncNow", "lastSynced",
"globalModels", "noGlobalModels", "modelSelection", "noModels"
```
