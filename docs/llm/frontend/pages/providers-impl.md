# Web -- Providers Page: Ollama Components

> SSOT | **Last Updated**: 2026-03-08 | Companion to `providers.md`

## OllamaServerMetrics

Component `OllamaServerMetrics({ serverId, gpuIndex })` renders below GPU/VRAM info for rows with a linked server.

- Query: `['server-metrics', serverId]` via `api.serverMetrics(serverId)`, `refetchInterval: 30_000`, `retry: false`
- Displays (compact line): `MEM used/total`, temp (red >=85, amber >=70, grey otherwise), power watts
- If `scrape_ok === false` or error: italic `"unreachable"` in red. Hidden when no server linked.

## OllamaProviderModelsModal

Opened by Model Selection on a provider row. Switch toggle UI per synced model.

| Aspect | Detail |
|--------|--------|
| Data | `GET /v1/providers/{id}/selected-models` -- `ollama_models` merged with `provider_selected_models`, default `is_enabled = true` |
| Toggle | `PATCH /v1/providers/{id}/selected-models/{model_name}` `{ is_enabled: bool }` |
| Query key | `['selected-models', providerId]` |
| Update | Optimistic: switch flips immediately, reverts on error |
| Empty state | `providers.ollama.noProviderModels` |
| Enabled count | `providers.ollama.enabledCount` (`X/Y enabled`) |

---

## OllamaSyncSection -- Global Model Sync

| Query | Key | Options |
|-------|-----|---------|
| Sync job | `['ollama-sync-status']` via `api.ollamaSyncStatus` | `refetchInterval`: 2000 when running, else false; `retry: false` |
| Models | `['ollama-models']` via `api.ollamaModels` | `staleTime: 30_000` |

- **Sync All**: `POST /v1/ollama/models/sync` -- invalidates `['ollama-sync-status']` + `['ollama-models']`
- Button disabled while running
- Model list: searchable, filtered client-side, shows filtered/total count
- Each row clickable -- opens `OllamaModelProvidersModal`

## OllamaModelProvidersModal

| Aspect | Detail |
|--------|--------|
| Query key | `['ollama-model-providers', modelName]`, `staleTime: 30_000` |
| Endpoint | `GET /v1/ollama/models/{model_name}/providers` |
| Pagination | `PAGE_SIZE = 8`; Prev/Next; page resets when search changes |
| Search | Filters by name OR url (host portion) |
| Status | Dot + badge: green=online, amber=degraded, red=offline |

---

## OllamaCapacitySection -- VRAM Pool View

No props. Placed after `<OllamaSyncSection />` in OllamaTab.

| Type | Key | Endpoint |
|------|-----|----------|
| Query | `['capacity']` | `GET /v1/dashboard/capacity` |
| Query | `['sync-settings']` | `GET /v1/dashboard/capacity/settings` |
| Mutation | `patchSyncSettings` | `PATCH /v1/dashboard/capacity/settings` |
| Mutation | `syncAllProviders` | `POST /v1/providers/sync` |

**Settings card**:

| Field | Detail |
|-------|--------|
| `providerFilter` | `<select>` filters analyzer model list by provider type (all/ollama/gemini); Gemini hidden when `gemini_function_calling` lab feature disabled |
| `analyzerModel` | `<select>` from `settings.available_models` grouped by provider type (Ollama/Gemini). Backend: Ollama via `/api/tags`, Gemini via `gemini_models` DB with Gemini API fallback when DB empty |
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
