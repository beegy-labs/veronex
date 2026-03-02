# Web — Providers Page (/providers)

> SSOT | **Last Updated**: 2026-03-02 (Ollama model enable/disable — OllamaBackendModelsModal → Switch toggle UI)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new Gemini query key | `web/app/providers/page.tsx` `GEMINI_QUERY_KEYS` constant | Add key → use in query + add to `refreshGeminiData()` |
| Change capacity analyzer model options | `web/app/providers/page.tsx` `OllamaCapacitySection` — `<select>` maps `available_models` | Models come from `GET /v1/dashboard/capacity/settings` `available_models` (Ollama /api/tags) |
| Change capacity refresh interval | `web/app/providers/page.tsx` `OllamaCapacitySection` — `useQuery` capacity query | Default: no auto-refetch (manual Sync Now) |
| Add action button to Ollama backend row | `web/app/providers/page.tsx` `OllamaTab` row actions | Same pattern as existing actions |
| Add action button to Gemini paid backend row | `web/app/providers/page.tsx` `GeminiTab` Gemini API Keys Table | Paid vs free tier conditional (`!backend.is_free_tier`) |
| Add field to RegisterOllamaModal | `web/app/providers/page.tsx` modal form state + `web/lib/api.ts` `registerBackend()` | Add field → pass to `api.registerBackend(body)` |
| Change OllamaSyncSection empty state | `web/app/providers/page.tsx` `OllamaSyncSection` + `web/messages/en.json` `backends.ollama.ollamaNoSync` | Update i18n key in all 3 locales |
| Change table page size | `web/app/providers/page.tsx` `PAGE_SIZE` constant | Single constant used by all 3 tables |
| Change rate limit policy table columns | `web/app/providers/page.tsx` `GeminiSyncSection` table | Add/remove column header + cell render |
| Change ModelSelectionModal empty state | `web/app/providers/page.tsx` `ModelSelectionModal` + `web/messages/en.json` `backends.noGlobalModels` | Update i18n key in all 3 locales |
| Change OllamaBackendModelsModal empty state | `web/app/providers/page.tsx` `OllamaBackendModelsModal` + `web/messages/en.json` `backends.ollama.noBackendModels` | Update i18n key in all 3 locales |
| Change Ollama live metrics refresh interval | `web/app/providers/page.tsx` `OllamaServerMetrics` `refetchInterval` | Default: 30 000 ms |
| Add live metric field to Ollama server cell | `web/app/providers/page.tsx` `OllamaServerMetrics` render | Add field from `NodeMetrics.gpus[n]` |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/providers/page.tsx` | All 2 tabs + all modals (OllamaTab, GeminiTab) |
| `web/lib/api.ts` | `api.servers()`, `api.backends()`, `api.ollamaModels()`, `api.syncOllamaModels()`, `api.ollamaSyncStatus()`, `api.geminiModels()`, `api.syncGeminiStatus()`, `api.capacity()`, `api.capacitySettings()`, `api.patchCapacitySettings()`, `api.triggerCapacitySync()` |
| `web/lib/types.ts` | `LlmBackend`, `GpuServer`, `OllamaSyncJob`, `GeminiRateLimitPolicy`, `GeminiModel`, `BackendSelectedModel`, `GeminiStatusSyncResponse`, `CapacityResponse`, `BackendCapacityInfo`, `ModelCapacityInfo`, `CapacitySettings`, `PatchCapacitySettings` |
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

**OllamaTab renders (in order)**:
1. Status pills + Register button
2. Backend table with pagination
3. `<OllamaSyncSection />` — global model sync
4. `<OllamaCapacitySection />` — concurrency control (see below)
5. Modals: `OllamaBackendModelsModal`, `ServerHistoryModal`

Actions: [↻ Healthcheck] [🔄 Sync Models] [⊞ Model Selection (`ListFilter`)] [✏️ Edit] [🗑 Delete]

- **[🔄 Sync Models]**: `POST /v1/backends/{id}/models/sync` — persists to `ollama_models` + upserts `backend_selected_models` (`is_enabled=true` for new rows). Invalidates `['ollama-sync-status']`, `['ollama-models']`, `['selected-models', backendId]`.
- **[⊞ Model Selection]** (ListFilter icon): opens `OllamaBackendModelsModal` — Switch toggle per model, same pattern as Gemini `ModelSelectionModal`.

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

Opened by [⊞ Model Selection] on a backend row. Switch toggle UI per synced model — identical pattern to Gemini `ModelSelectionModal`.

- Data: `GET /v1/backends/{id}/selected-models` → Ollama branch: `ollama_models` merged with `backend_selected_models`, default `is_enabled = true`
- Toggle → `PATCH /v1/backends/{id}/selected-models/{model_name}` `{ is_enabled: bool }`
- `queryKey: ['selected-models', backendId]`
- Optimistic update: switch flips immediately, reverts on error
- Empty state: `backends.ollama.noBackendModels` (no models synced yet)
- Enabled count: `backends.ollama.enabledCount` (`X/Y enabled`)

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

### OllamaCapacitySection — Concurrency Control

Defined as `OllamaCapacitySection` component (no props). Placed after `<OllamaSyncSection />` in OllamaTab.

```
┌─────────────────────────────────────────────────────────────────────┐
│ ⚡ Concurrency Control                                               │
│   VRAM-aware slot allocation for local Ollama inference             │
│                                                                      │
│ ┌── Analyzer Settings ───────────────────────────────────────────┐  │
│ │ Analyzer Model [qwen2.5:3b ▾]  Auto Analysis [●──]             │  │
│ │ Interval (s)   [300       ]    Last run: Mar 2 12:34  ✓ ok     │  │
│ │ [Save]                         [Sync Now ↻]                    │  │
│ └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ollama-rtx4090  🌡 Normal  72°C                                    │
│  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄  │
│  Model         Slots  Active  VRAM(model)  KV/slot  TPS  P95       │
│  llama3.2:3b    3      1/3    2.0 GB       56 MB    48   4.3s      │
│  qwen2.5:7b     2      0/2    4.5 GB       112 MB   —    —         │
│  [!] High VRAM pressure at current temperature — reduce slots       │
└─────────────────────────────────────────────────────────────────────┘
```

**Queries**:
```typescript
useQuery({ queryKey: ['capacity'], queryFn: api.capacity })         // GET /v1/dashboard/capacity
useQuery({ queryKey: ['capacity-settings'], queryFn: api.capacitySettings })  // GET /v1/dashboard/capacity/settings
```

**Mutations**:
```typescript
useMutation({ mutationFn: api.patchCapacitySettings })  // PATCH /v1/dashboard/capacity/settings
useMutation({ mutationFn: api.triggerCapacitySync })    // POST  /v1/dashboard/capacity/sync
```

**Settings card behaviour**:
- `analyzerModel` — `<select>` populated from `settings.available_models` (Ollama /api/tags)
- `batchEnabled` — Switch; when off, auto-analysis loop is paused (manual sync still works)
- `intervalSecs` — number input (min: 60, step: 30); next loop cycle picks up new value
- **Save**: `PATCH /v1/dashboard/capacity/settings` → invalidates `['capacity-settings']`
- **Sync Now**: `POST /v1/dashboard/capacity/sync` → toast "Analysis triggered" → invalidates `['capacity', 'capacity-settings']` after 3s delay (analysis runs async on server)

**Capacity table** (per backend → per loaded model):
- `ThermalBadge`: `normal` = green pill / `soft` = amber "Soft Throttle" / `hard` = red "Hard Throttle"
- `temp_c` shown alongside badge when non-null
- `recommended_slots` shown as bold circle badge (`⬤ N`)
- `active_slots / recommended_slots` format
- VRAM and KV/slot use `fmtMbShort(mb)` helper: `>= 1024` → `N.N GB`, else `N MB`
- `avg_tokens_per_sec`, `p95_latency_ms` shown as `—` when `sample_count === 0`
- When `llm_concern` is not null: extra row with yellow background showing concern + reason text
- Empty state card with "Sync Now" hint when `capacity.backends` is empty

**Helpers**:
```typescript
function ThermalBadge({ state }: { state: 'normal' | 'soft' | 'hard' })  // colored pill
function fmtMbShort(mb: number): string  // >= 1024 → "N.N GB", else "N MB"
```

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

#### SSOT: `GEMINI_QUERY_KEYS`

Module-level constant — **all** Gemini query key references must use this:

```typescript
const GEMINI_QUERY_KEYS = {
  syncConfig:     ['gemini-sync-config'],
  models:         ['gemini-models'],
  policies:       ['gemini-policies'],
  selectedModels: ['selected-models'], // prefix — matches all ['selected-models', backendId]
} as const
```

`refreshGeminiData()` is the single refresh function inside `GeminiSyncSection`:
```typescript
function refreshGeminiData() {
  queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.models })
  queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.policies })
  queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.selectedModels })
}
```
Both the **Sync Now** button (`syncMutation.onSuccess`) and the **Refresh** button call this.
When sync completes, `['selected-models', *]` is also invalidated so `ModelSelectionModal`
picks up newly synced models automatically.

**State** (inside GeminiSyncSection):
```typescript
const { data: modelsData, isFetching: modelsFetching } = useQuery({
  queryKey: GEMINI_QUERY_KEYS.models, queryFn: api.geminiModels })
const { data: policies, isFetching: policiesFetching } = useQuery({
  queryKey: GEMINI_QUERY_KEYS.policies, queryFn: api.geminiPolicies })
const { data: syncConfig } = useQuery({
  queryKey: GEMINI_QUERY_KEYS.syncConfig, queryFn: api.geminiSyncConfig })
const isRefreshing = (modelsFetching || policiesFetching) && !syncMutation.isPending
```

**Sync Controls Card**: Admin API Key (masked) + Edit → `SetSyncKeyModal`

```
[Sync Now ↻]   [↻ Refresh]   Last synced: Mar 1 08:49   ✓ 12 global models
```

- **Sync Now**: calls `POST /v1/gemini/models/sync` → Gemini API → DB → `refreshGeminiData()`
- **Refresh**: calls `refreshGeminiData()` only (DB re-read, no Gemini API call)
- Refresh button shows spinner (`animate-spin`) when `isRefreshing`

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

**SetSyncKeyModal**: password input → `PUT /v1/gemini/sync-config`. Invalidates `GEMINI_QUERY_KEYS.syncConfig`.

### 4. ModelSelectionModal (paid Gemini backends)

Opened by `ListFilter` button on paid backend rows.

- Data: `GET /v1/backends/{id}/selected-models` → Gemini branch: global `gemini_models` merged with per-backend state, default `is_enabled = false`
- Toggle → `PATCH /v1/backends/{id}/selected-models/{model_name}` `{ is_enabled: bool }`
- Optimistic update: switch flips immediately, reverts on error
- Empty state: "No global models. Set an admin key and click Sync Now."
- `useQuery({ queryKey: [...GEMINI_QUERY_KEYS.selectedModels, backendId] })`
- Auto-refreshed when `refreshGeminiData()` is called (prefix invalidation)

> **Ollama counterpart**: `OllamaBackendModelsModal` — same Switch UI, same endpoint, Ollama branch returns per-backend models with `is_enabled = true` default. See above.

---

## i18n Keys (messages/en.json → `backends.*`)

```json
"registerServer", "editServer", "serverName", "nodeExporterUrl",
"registerOllama", "editOllama", "backendUrl", "totalVram", "gpuIndex",
"ollama.ollamaSyncSection", "ollama.ollamaSyncAll", "ollama.ollamaSyncing",
"ollama.ollamaSyncDone", "ollama.ollamaAvailableModels", "ollama.ollamaNoSync",
"ollama.ollamaSearchModels", "ollama.viewModels",
"ollama.modelSelection", "ollama.modelSelectionDesc", "ollama.enabledCount",
"registerGemini", "editGemini", "apiKey", "freeTier",
"gemini.statusSyncSection", "gemini.statusSyncDesc", "gemini.syncStatus",
"gemini.syncingStatus", "gemini.statusSyncDone", "gemini.noStatusResults",
"syncSection", "syncKey", "setSyncKey", "syncNow", "lastSynced",
"globalModels", "noGlobalModels", "modelSelection", "noModels",
"capacity.title", "capacity.desc", "capacity.syncNow", "capacity.syncing",
"capacity.triggered", "capacity.settings", "capacity.analyzerModel",
"capacity.autoAnalysis", "capacity.interval", "capacity.saving",
"capacity.lastRun", "capacity.never", "capacity.statusOk", "capacity.statusError",
"capacity.noData", "capacity.slots", "capacity.recommended",
"capacity.vramModel", "capacity.kvPerSlot", "capacity.avgTps", "capacity.p95",
"capacity.sampleCount", "capacity.thermal.normal", "capacity.thermal.soft",
"capacity.thermal.hard", "capacity.concern", "capacity.reason"
```
