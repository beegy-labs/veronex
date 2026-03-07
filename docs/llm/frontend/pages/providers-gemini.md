# Web -- Providers Page: Gemini Components

> SSOT | **Last Updated**: 2026-03-04 | Companion to `providers.md`

## GeminiSyncSection -- Global Model Sync + Rate Limits

### GEMINI_QUERY_KEYS

Module-level constant -- all Gemini query key references must use this:

```typescript
const GEMINI_QUERY_KEYS = {
  syncConfig:     ['gemini-sync-config'],
  models:         ['gemini-models'],
  policies:       ['gemini-policies'],
  selectedModels: ['selected-models'],
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

Both Sync Now (`syncMutation.onSuccess`) and Refresh button call this. When sync completes, `['selected-models', *]` is also invalidated so `ModelSelectionModal` picks up newly synced models.

**State** (inside GeminiSyncSection):

| Query | Key | API |
|-------|-----|-----|
| Models | `GEMINI_QUERY_KEYS.models` | `api.geminiModels` |
| Policies | `GEMINI_QUERY_KEYS.policies` | `api.geminiPolicies` |
| Sync config | `GEMINI_QUERY_KEYS.syncConfig` | `api.geminiSyncConfig` |

`isRefreshing = (modelsFetching || policiesFetching) && !syncMutation.isPending`

**Sync Controls Card**: Admin API Key (masked) + Edit via `SetSyncKeyModal`.

- **Sync Now**: `POST /v1/gemini/models/sync` -- Gemini API call + DB write + `refreshGeminiData()`
- **Refresh**: `refreshGeminiData()` only (DB re-read, no Gemini API call)
- Refresh button shows spinner (`animate-spin`) when `isRefreshing`

### Rate Limit Table

Per-model policies only -- no global `*` default row displayed.

| Row state | Display |
|-----------|---------|
| Model with specific policy | Shown normally |
| Model without specific policy | `opacity-60`, `global default` label, date = `-` |
| Global `*` default | Not displayed (DB only, routing fallback) |

**EditPolicyModal**: Model name (read-only), Free Tier toggle (Switch). When on: RPM/RPD inputs visible. When off: inputs hidden (paid-only). Save via `api.upsertGeminiPolicy(model_name, request)`.

**SetSyncKeyModal**: Password input -- `PUT /v1/gemini/sync-config`. Invalidates `GEMINI_QUERY_KEYS.syncConfig`.

---

## ModelSelectionModal (paid Gemini providers)

Opened by `ListFilter` button on paid provider rows.

| Aspect | Detail |
|--------|--------|
| Data | `GET /v1/providers/{id}/selected-models` -- global `gemini_models` merged with per-provider state, default `is_enabled = false` |
| Toggle | `PATCH /v1/providers/{id}/selected-models/{model_name}` `{ is_enabled: bool }` |
| Update | Optimistic: switch flips immediately, reverts on error |
| Empty state | "No global models. Set an admin key and click Sync Now." |
| Query key | `[...GEMINI_QUERY_KEYS.selectedModels, providerId]` |
| Refresh | Auto-refreshed when `refreshGeminiData()` called (prefix invalidation) |

Ollama counterpart: `OllamaProviderModelsModal` -- same Switch UI, same endpoint, Ollama branch returns per-provider models with `is_enabled = true` default.

---

## i18n Keys (messages/en.json -- `providers.*`)

Gemini-specific keys live under `providers.gemini.*`. Shared provider-level keys are under `providers.*`.

```json
"providers.gemini.title", "providers.gemini.name", "providers.gemini.apiKey",
"providers.gemini.freeTier", "providers.gemini.status", "providers.gemini.activeToggle",
"providers.gemini.models", "providers.gemini.noProviders", "providers.gemini.noProvidersHint",
"providers.gemini.registerProvider", "providers.gemini.registerTitle", "providers.gemini.editTitle",
"providers.gemini.rateLimitPolicies", "providers.gemini.rateLimitDesc",
"providers.gemini.model", "providers.gemini.rpm", "providers.gemini.rpd",
"providers.gemini.onFreeTier", "providers.gemini.noPolicies",
"providers.gemini.loadingProviders", "providers.gemini.loadingPolicies",
"providers.gemini.failedProviders", "providers.gemini.providerMeta",
"providers.gemini.paid", "providers.gemini.enabled", "providers.gemini.paidOnly",
"providers.gemini.globalDefault", "providers.gemini.lastUpdated",
"providers.gemini.editPolicyTitle", "providers.gemini.availableOnFreeTier",
"providers.gemini.freeTierRouting", "providers.gemini.paidOnlyRouting",
"providers.gemini.freeLimitsHint", "providers.gemini.failedToSave",
"providers.gemini.modelSelection", "providers.gemini.syncModels",
"providers.gemini.modelSelectionDesc", "providers.gemini.noSyncedModels",
"providers.gemini.noGlobalModels", "providers.gemini.modelsCount",
"providers.gemini.syncKeyHint", "providers.gemini.freeTierDesc",
"providers.gemini.apiKeyHint", "providers.gemini.keepExistingKey",
"providers.gemini.globalFallbackHint",
"providers.gemini.syncSection", "providers.gemini.syncSectionDesc",
"providers.gemini.syncKey", "providers.gemini.setSyncKey", "providers.gemini.noSyncKey",
"providers.gemini.syncNow", "providers.gemini.lastSynced", "providers.gemini.globalModels",
"providers.gemini.statusSyncSection", "providers.gemini.statusSyncDesc",
"providers.gemini.syncStatus", "providers.gemini.syncingStatus",
"providers.gemini.statusSyncDone", "providers.gemini.noStatusResults",
"providers.capacity.*"
```
