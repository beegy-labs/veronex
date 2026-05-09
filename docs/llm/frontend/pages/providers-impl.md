# Web -- Providers Page: llama-server Components

> SSOT | **Last Updated**: 2026-03-08 | Companion to `providers.md`

## LlamaServerMetrics

Component `LlamaServerMetrics({ serverId, gpuIndex })` renders below GPU/VRAM info for rows with a linked server.

- Query: `['server-metrics', serverId]` via `api.serverMetrics(serverId)`, `refetchInterval: 30_000`, `retry: false`
- Displays (compact line): `MEM used/total`, temp (red >=85, amber >=70, grey otherwise), power watts
- If `scrape_ok === false` or error: italic `"unreachable"` in red. Hidden when no server linked.

## LlamaServerProviderModelsModal

Opened by Model Selection on a provider row. Switch toggle UI per synced model.

| Aspect | Detail |
|--------|--------|
| Data | `GET /v1/providers/{id}/selected-models` -- `llama_server_models` merged with `provider_selected_models`, default `is_enabled = true` |
| Toggle | `PATCH /v1/providers/{id}/selected-models/{model_name}` `{ is_enabled: bool }` |
| Query key | `['selected-models', providerId]` |
| Update | Optimistic: switch flips immediately, reverts on error |
| Empty state | `providers.llama-server.noProviderModels` |
| Enabled count | `providers.llama-server.enabledCount` (`X/Y enabled`) |

---

## LlamaServerSyncSection -- Global Model Sync

| Query | Key | Options |
|-------|-----|---------|
| Sync job | `['llama-server-sync-status']` via `api.llamaServerSyncStatus` | `refetchInterval`: 2000 when running, else false; `retry: false` |
| Models | `['llama-server-models']` via `(removed)` | `staleTime: 30_000` |

- **Sync All**: `POST /v1/llama-server/models/sync` -- invalidates `['llama-server-sync-status']` + `['llama-server-models']`
- Button disabled while running
- Model list: searchable, filtered client-side, shows filtered/total count
- Each row clickable -- opens `LlamaServerModelProvidersModal`

## LlamaServerModelProvidersModal

| Aspect | Detail |
|--------|--------|
| Query key | `['llama-server-model-providers', modelName]`, `staleTime: 30_000` |
| Endpoint | `GET /v1/llama-server/models/{model_name}/providers` |
| Pagination | `PAGE_SIZE = 8`; Prev/Next; page resets when search changes |
| Search | Filters by name OR url (host portion) |
| Status | Dot + badge: green=online, amber=degraded, red=offline |

---

## LlamaServerCapacitySection -- VRAM Pool View

No props. Placed after `<LlamaServerSyncSection />` in LlamaServerTab.

| Type | Key | Endpoint |
|------|-----|----------|
| Query | `['capacity']` | `GET /v1/dashboard/capacity` |
| Query | `['sync-settings']` | `GET /v1/dashboard/capacity/settings` |
| Mutation | `patchSyncSettings` | `PATCH /v1/dashboard/capacity/settings` |
| Mutation | `syncAllProviders` | `POST /v1/providers/sync` |

**Settings card**:

| Field | Detail |
|-------|--------|
| `providerFilter` | `<select>` filters analyzer model list by provider type (all/llama-server/gemini); Gemini hidden when `gemini_function_calling` lab feature disabled |
| `analyzerModel` | `<select>` from `settings.available_models` grouped by provider type (llama-server/Gemini). Backend: llama-server via `/api/tags`, Gemini via `gemini_models` DB with Gemini API fallback when DB empty |
| `syncEnabled` | Switch; off = auto-sync paused (manual sync still works) |
| `syncIntervalSecs` | Number input (min: 60, step: 30) |
| `probePermits` | Number input; AIMD probe: +N (probe up), -N (probe down), 0=disabled |
| `probeRate` | Number input (min: 0); 1 probe per N limit hits |
| Save | Invalidates `['sync-settings']` |
| Sync Now | Toast "Sync triggered" -- invalidates `['capacity', 'sync-settings']` after 3s delay |

**VRAM Pool view** (per provider):

| Column | Format |
|--------|--------|
| Thermal | `ThermalBadge`: normal=green, soft=amber, hard=red; `temp_c` alongside |
| VRAM Bar | Progress bar: used/total, `fmtMbShort(mb)` labels |
| Loaded models | List: model_name, weight_mb, kv/request, active/limit (AIMD) |
| Concern | When `llm_concern` not null: yellow row with concern + reason |
| Empty | Card with "Sync Now" hint when `capacity.providers` empty |

Helpers: `ThermalBadge({ state })` colored pill, `VramBar({ used, total })` progress, `fmtMbShort(mb)` size formatter.
