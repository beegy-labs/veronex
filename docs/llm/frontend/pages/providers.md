# Web -- Providers Page (/providers)

> SSOT | **Last Updated**: 2026-03-04 | See `providers-impl.md` (Ollama components), `providers-gemini.md` (Gemini components)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new Gemini query key | `web/app/providers/page.tsx` `GEMINI_QUERY_KEYS` constant | Add key, use in query + add to `refreshGeminiData()` |
| Change capacity analyzer model options | `web/app/providers/page.tsx` `OllamaCapacitySection` -- `<select>` maps `available_models` | Models come from `GET /v1/dashboard/capacity/settings` `available_models` (Ollama /api/tags) |
| Change capacity refresh interval | `web/app/providers/page.tsx` `OllamaCapacitySection` -- `useQuery` capacity query | Default: no auto-refetch (manual Sync Now) |
| Add action button to Ollama provider row | `web/app/providers/page.tsx` `OllamaTab` row actions | Same pattern as existing actions |
| Add action button to Gemini paid provider row | `web/app/providers/page.tsx` `GeminiTab` Gemini API Keys Table | Paid vs free tier conditional (`!provider.is_free_tier`) |
| Add field to RegisterOllamaModal | `web/app/providers/page.tsx` modal form state + `web/lib/api.ts` `registerProvider()` | Add field, pass to `api.registerProvider(body)` |
| Change OllamaSyncSection empty state | `web/app/providers/page.tsx` `OllamaSyncSection` + `web/messages/en.json` `providers.ollama.ollamaNoSync` | Update i18n key in all 3 locales |
| Change table page size | `web/app/providers/page.tsx` `PAGE_SIZE` constant | Single constant used by all 3 tables |
| Change rate limit policy table columns | `web/app/providers/page.tsx` `GeminiSyncSection` table | Add/remove column header + cell render |
| Change ModelSelectionModal empty state | `web/app/providers/page.tsx` `ModelSelectionModal` + `web/messages/en.json` `providers.gemini.noGlobalModels` | Update i18n key in all 3 locales |
| Change OllamaProviderModelsModal empty state | `web/app/providers/page.tsx` `OllamaProviderModelsModal` + `web/messages/en.json` `providers.ollama.noProviderModels` | Update i18n key in all 3 locales |
| Change Ollama live metrics refresh interval | `web/app/providers/page.tsx` `OllamaServerMetrics` `refetchInterval` | Default: 30 000 ms |
| Add live metric field to Ollama server cell | `web/app/providers/page.tsx` `OllamaServerMetrics` render | Add field from `NodeMetrics.gpus[n]` |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/providers/page.tsx` | All 2 tabs + all modals (OllamaTab, GeminiTab) |
| `web/lib/api.ts` | `api.servers()`, `api.providers()`, `api.ollamaModels()`, `api.syncOllamaModels()`, `api.ollamaSyncStatus()`, `api.geminiModels()`, `api.syncGeminiStatus()`, `api.capacity()`, `api.capacitySettings()`, `api.patchCapacitySettings()`, `api.triggerCapacitySync()` |
| `web/lib/types.ts` | `Provider`, `GpuServer`, `OllamaSyncJob`, `GeminiRateLimitPolicy`, `GeminiModel`, `ProviderSelectedModel`, `GeminiStatusSyncResponse`, `CapacityResponse`, `ProviderCapacityInfo`, `ModelCapacityInfo`, `CapacitySettings`, `PatchCapacitySettings` |
| `web/messages/en.json` | i18n keys under `providers.*` |

---

## Routing

URL `?s=` param (default: `ollama`):

| URL | Section | Lab Gate |
|-----|---------|----------|
| `/providers` or `?s=ollama` | `OllamaTab` -- Ollama provider management | always visible |
| `?s=gemini` | `GeminiTab` -- Gemini + rate-limit policies | `gemini_function_calling` must be enabled |

Section switching via `<Link>` in `nav.tsx` -- no internal tab state in page.

Lab gating (`ProvidersContent`):
```typescript
const { labSettings } = useLabSettings()
const geminiEnabled = labSettings?.gemini_function_calling ?? false
const section = (sectionParam === 'gemini' && !geminiEnabled) ? 'ollama' : sectionParam
```

- When disabled: direct navigation to `?s=gemini` falls back to OllamaTab silently.
- The Gemini nav child item is also hidden in `nav.tsx` (filtered by `useLabSettings()`).
- Enable via Settings > Lab Features > "Gemini function calling" toggle.

Nav entry: `NavGroup` with `id: 'providers'`, `basePath: '/providers'`, `Server` icon.

---

## Pagination (all three provider tables)

`PAGE_SIZE = 10` (single constant, shared).

Each table (Ollama, Gemini, Servers) uses local `page` state. Pattern:

```typescript
const [page, setPage] = useState(1)
const totalPages = Math.max(1, Math.ceil(items.length / PAGE_SIZE))
const safePage = Math.min(page, totalPages)
const pageItems = items.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE)
```

Controls: range `{start}-{end} / {total}`, ChevronLeft/ChevronRight icon buttons (disabled at boundary), hidden when `totalPages <= 1`.

---

## OllamaTab -- Overview

Header shows pill badges: `N registered` / `N online` / `N offline`.

**OllamaTab renders (in order)**:
1. Status pills + Register button
2. Provider table with pagination
3. `<OllamaSyncSection />` -- global model sync
4. `<OllamaCapacitySection />` -- concurrency control
5. Modals: `OllamaProviderModelsModal`, `ServerHistoryModal`

Actions per row: Healthcheck, Sync Models, Model Selection (`ListFilter`), Edit, Delete.

- **Sync Models**: `POST /v1/providers/{id}/models/sync` -- persists to `ollama_models` + upserts `provider_selected_models` (`is_enabled=true` for new rows). Invalidates `['ollama-sync-status']`, `['ollama-models']`, `['selected-models', providerId]`.
- **Model Selection**: opens `OllamaProviderModelsModal` -- Switch toggle per model.

**RegisterOllamaModal fields**: name, URL, total_vram_mb, gpu_index, server_id (dropdown).
**EditModal fields**: name, URL, api_key (blank = keep existing), total_vram_mb, gpu_index, server_id.

See `providers-impl.md` for OllamaServerMetrics, OllamaProviderModelsModal, OllamaSyncSection, OllamaModelProvidersModal, OllamaCapacitySection details.

---

## GeminiTab -- Overview

Section header shows pill badges: `N registered` / `N active` / `N online` / `N degraded` / `N offline`.

Three sections:
1. **Gemini API Keys Table** -- provider registration + row actions
2. **GeminiStatusSyncSection** -- manual connectivity check
3. **GeminiSyncSection** -- global model sync + rate limit policies

### Gemini API Keys Table

Free tier actions: Healthcheck, Edit, Delete.
Paid tier actions (+): Model Selection (`ListFilter` icon).

**RegisterGeminiModal / EditModal**: name, api_key (blank on edit = keep), Free Tier toggle.

Gemini status is NOT auto-checked by the background health checker. Status is only updated via the GeminiStatusSyncSection "Sync Status" button or per-row Healthcheck button.

### GeminiStatusSyncSection

- `POST /v1/gemini/sync-status` -- returns `GeminiStatusSyncResponse { synced_at, results[{ id, name, status, error }] }`
- `onSuccess`: invalidates `['providers']`
- Result list shown after sync; empty when no providers

See `providers-gemini.md` for GeminiSyncSection, ModelSelectionModal, and i18n keys.
