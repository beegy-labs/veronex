# Providers -- Ollama: Global Model Sync & Model-Aware Routing

> SSOT | **Last Updated**: 2026-04-06

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Trigger global sync of all Ollama providers | `POST /v1/ollama/models/sync` | `ollama_model_handlers.rs` -> `sync_all_providers()` |
| Poll sync progress | `GET /v1/ollama/sync/status` | `ollama_model_handlers.rs` -> `get_sync_status()` |
| Get global model pool (with counts) | `GET /v1/ollama/models` | `ollama_model_handlers.rs` -> `list_models()` -> `list_with_counts()` |
| Get providers for a model | `GET /v1/ollama/models/{model}/providers` | `ollama_model_handlers.rs` -> `list_model_providers()` |
| Get models for a provider | `GET /v1/ollama/providers/{id}/models` | `ollama_model_handlers.rs` -> `list_provider_models()` |
| Per-provider sync (also updates DB) | `POST /v1/providers/{id}/models/sync` | `provider_handlers.rs` -> `sync_provider_models()` (Ollama path) |
| Change model-aware routing filter | `provider_router.rs` -> `pick_best_provider()` | Modify `ollama_model_repo` filter block |
| Toggle a model on/off for a provider | `PATCH /v1/providers/{id}/selected-models/{model}` | `set_model_enabled()` in `provider_handlers.rs` |
| Change model selection default on sync | `provider_handlers.rs` + `ollama_model_handlers.rs` | `upsert_models()` inserts `is_enabled = true` for new rows |
| Change modal pagination size | `web/app/providers/page.tsx` `PROVIDERS_PAGE_SIZE` | Affects `OllamaModelProvidersModal` |
| Add field to OllamaModel | `docker/postgres/init.sql` + `ollama_model_repository.rs` (port + pg impl) | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/ollama_model_handlers.rs` | Global sync + status + model list handlers |
| `crates/veronex/src/application/ports/outbound/ollama_model_repository.rs` | `OllamaModelRepository` trait |
| `crates/veronex/src/application/ports/outbound/ollama_sync_job_repository.rs` | `OllamaSyncJobRepository` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/ollama_model_repository.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/outbound/persistence/ollama_sync_job_repository.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/inbound/http/provider_handlers.rs` | `sync_provider_models` (Ollama path) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `pick_best_provider()` -- model-aware filter |

---

## DB Schema

```sql
CREATE TABLE ollama_models (
  model_name  TEXT NOT NULL,
  provider_id UUID NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
  synced_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (model_name, provider_id)
);

CREATE TABLE ollama_sync_jobs (
  id               UUID        PRIMARY KEY DEFAULT uuidv7(),
  started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  completed_at     TIMESTAMPTZ,
  status           TEXT        NOT NULL DEFAULT 'running',  -- 'running' | 'completed'
  total_providers  INT         NOT NULL DEFAULT 0,
  done_providers   INT         NOT NULL DEFAULT 0,
  results          JSONB       NOT NULL DEFAULT '[]'::jsonb
);
```

---

## Ports (application/ports/outbound/)

### Domain Structs

| Struct | Fields |
|--------|--------|
| `OllamaModel` | `model_name: String`, `provider_id: Uuid`, `synced_at: DateTime<Utc>` |
| `OllamaModelWithCount` | `model_name: String`, `provider_count: i64`, `max_ctx: i32` |
| `OllamaProviderForModel` | `provider_id: Uuid`, `name: String`, `url: String`, `status: String`, `is_enabled: bool` |
| `OllamaSyncJob` | `id: Uuid`, `started_at`, `completed_at: Option`, `status: String`, `total_providers: i32`, `done_providers: i32`, `results: serde_json::Value` |

### Trait Methods

| Trait | Method | Returns |
|-------|--------|---------|
| `OllamaModelRepository` | `sync_provider_models(provider_id, &[String])` | `Result<()>` -- atomic DELETE + INSERT |
| | `list_all()` | `Result<Vec<String>>` -- legacy, prefer `list_with_counts` |
| | `list_with_counts_page(search, limit, offset)` | `Result<(Vec<OllamaModelWithCount>, i64)>` -- paginated, ILIKE search |
| | `providers_for_model(model_name)` | `Result<Vec<Uuid>>` -- used for routing |
| | `providers_info_for_model_page(model_name, limit, offset)` | `Result<(Vec<OllamaProviderForModel>, i64)>` -- paginated, includes is_enabled from provider_selected_models |
| | `models_for_provider(provider_id)` | `Result<Vec<String>>` -- used by UI |
| `OllamaSyncJobRepository` | `create(total_providers)` | `Result<Uuid>` |
| | `update_progress(id, result: Value)` | `Result<()>` -- appends result, increments done |
| | `complete(id)` | `Result<()>` |
| | `get_latest()` | `Result<Option<OllamaSyncJob>>` |

---

## API Endpoints (ollama_model_handlers.rs)

### Global Model Pool

```
GET /v1/ollama/models?search=&page=1&limit=20
  -> { models: [{ model_name: "llama3", provider_count: 3, is_vision: false, max_ctx: 131072 }, ...], total: N, page: 1, limit: 20 }
  Defaults: limit=20, max=200
```

`is_vision` — derived from model name heuristic (`is_vision_model()` in `inference_helpers.rs`). True for known vision model name patterns.
`max_ctx` — `MAX(model_vram_profiles.max_ctx)` across all providers. `0` = not yet profiled by capacity analyzer. Populated by capacity analyzer from Ollama `/api/show` response `context_length`. Schema: `docker/postgres/init.sql`.

Used by frontend context-window warnings: `getMultiturnWarnings()` in `api-test-form.tsx` uses `max_ctx` to warn when conversation token estimate exceeds 85% of model's context window.

### Global Sync (async background)

```
POST /v1/ollama/models/sync
  1. List all active Ollama providers from registry
  2. No active providers -> 400
  3. Create sync job -> tokio::spawn background task (sequential, no retry):
       for each provider:
         GET {provider.url}/api/tags -> parse model names
         sync_provider_models + upsert_models (is_enabled=true for new rows, non-fatal)
         update_progress(job_id, { provider_id, name, models, error })
         On failure: log error, continue to next provider
       complete(job_id)
  4. Return 202: { job_id: "uuid", status: "running" }
```

### Sync Status

```
GET /v1/ollama/sync/status -> OllamaSyncJob as JSON, 404 if never run

Example (completed with one failure):
{ "status": "completed", "total_providers": 3, "done_providers": 3,
  "results": [
    { "provider_id": "...", "name": "gpu-ollama-1", "models": ["llama3"], "error": null },
    { "provider_id": "...", "name": "gpu-ollama-2", "models": [], "error": "Connection refused" }
  ] }
```

### Per-Model / Per-Provider Lookups

| Endpoint | Response |
|----------|----------|
| `GET /v1/ollama/models/{model}/providers?page=1&limit=10` | `{ providers: [{ provider_id, name, url, status, is_enabled }], total: N, page: 1, limit: 10 }` |
| `GET /v1/ollama/providers/{id}/models` | `{ models: ["codellama", "llama3", ...] }` -- sorted |

### Per-Provider Sync (provider_handlers.rs)

```
POST /v1/providers/{id}/models/sync
  1. Fetch GET /api/tags from provider
  2. Cache in Valkey (TTL 1h)
  3. sync_provider_models(id, &models) -> updates ollama_models table
  4. upsert_models(id, &models) (non-fatal) -> is_enabled=true for new rows
  5. Returns { models, synced: true }
```

---

### Global Model Settings

Globally disable a model across all providers (Stage 0 dispatcher gate).
When `is_enabled = false`, the model is blocked regardless of per-provider `selected_models` state.

| Endpoint | Auth | Response |
|----------|------|----------|
| `GET /v1/models/global-settings` | `RequireModelManage` | `Vec<{ model_name, is_enabled, updated_at }>` |
| `GET /v1/models/global-disabled` | `RequireModelManage` | `Vec<String>` — model names where is_enabled = false |
| `PATCH /v1/models/global-settings/{model_name}` | `RequireModelManage` | `{ is_enabled: bool }` → 200 |

DB: `global_model_settings (model_name TEXT PK, is_enabled BOOL, updated_at TIMESTAMPTZ)` — `docker/postgres/init.sql`.

Permission: `model_manage` (9th permission in `ALL_PERMISSIONS`).

---

## Model-Aware Routing

> Full VRAM-aware routing + thermal throttle spec: `docs/llm/providers/ollama.md`

`pick_best_provider()` applies two sequential filters for Ollama dispatch:

| Filter | Source | Logic |
|--------|--------|-------|
| Model presence | `ollama_model_repo.providers_for_model()` | Non-empty -> intersect with candidates; empty (not yet synced) -> use all (fallback) |
| Model selection | `model_selection_repo.list_enabled()` | Non-empty list + model not in list -> skip provider; empty/error -> include (backward compat) |

After both filters, highest available VRAM wins.

Default after sync: all models `is_enabled = true`. Disable via `PATCH /v1/providers/{id}/selected-models/{model}` with `{ is_enabled: false }`.

Fallbacks are intentional: new deployments with empty tables route without restriction.

> AppState wiring: `docs/llm/policies/architecture.md` -- Composition Root + Port Catalog

---

## Web UI

-> See `docs/llm/frontend/pages/providers.md` -> OllamaSyncSection
-> See `docs/llm/frontend/pages/api-test.md` -> Ollama global model pool
