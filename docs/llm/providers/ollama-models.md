# Providers — Ollama: Global Model Sync & Model-Aware Routing

> SSOT | **Last Updated**: 2026-03-03 (Ollama model enable/disable — upsert to provider_selected_models on sync; selection filter in router)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Trigger global sync of all Ollama providers | `POST /v1/ollama/models/sync` | `ollama_model_handlers.rs` → `sync_all_providers()` |
| Poll sync progress | `GET /v1/ollama/sync/status` | `ollama_model_handlers.rs` → `get_sync_status()` |
| Get global model pool (with counts) | `GET /v1/ollama/models` | `ollama_model_handlers.rs` → `list_models()` → `list_with_counts()` |
| Get providers for a model | `GET /v1/ollama/models/{model}/providers` | `ollama_model_handlers.rs` → `list_model_providers()` |
| Get models for a provider | `GET /v1/ollama/providers/{id}/models` | `ollama_model_handlers.rs` → `list_provider_models()` |
| Per-provider sync (also updates DB) | `POST /v1/providers/{id}/models/sync` | `backend_handlers.rs` → `sync_provider_models()` (Ollama path) |
| Change model-aware routing filter | `infrastructure/outbound/provider_router.rs` → `pick_best_provider()` | Modify `ollama_model_repo` filter block |
| Toggle a model on/off for a provider | `PATCH /v1/providers/{id}/selected-models/{model}` | `set_model_enabled()` in `backend_handlers.rs` → `provider_selected_models` |
| Change model selection default on sync | `backend_handlers.rs` → `sync_provider_models()` + `ollama_model_handlers.rs` → `sync_all_providers()` | `upsert_models()` inserts `is_enabled = true` for new rows |
| Change modal pagination size | `web/app/providers/page.tsx` `PROVIDERS_PAGE_SIZE` | Affects `OllamaModelProvidersModal` |
| Add field to OllamaModel | `migrations/` + `ollama_model_repository.rs` (port + pg impl) | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/ollama_model_handlers.rs` | Global sync + status + model list handlers |
| `crates/veronex/src/application/ports/outbound/ollama_model_repository.rs` | `OllamaModelRepository` trait |
| `crates/veronex/src/application/ports/outbound/ollama_sync_job_repository.rs` | `OllamaSyncJobRepository` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/ollama_model_repository.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/outbound/persistence/ollama_sync_job_repository.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/inbound/http/backend_handlers.rs` | `sync_provider_models` (Ollama path) — persists to `ollama_models` |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `pick_best_provider()` — model-aware Ollama candidate filter |
| `crates/veronex/src/infrastructure/inbound/http/state.rs` | `AppState` fields |

---

## DB Schema

```sql
-- Per-provider model list (primary key on pair)
CREATE TABLE ollama_models (
    model_name  TEXT NOT NULL,
    provider_id UUID NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    synced_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (model_name, provider_id)
);

-- Global sync job tracking (persists progress across page navigation)
CREATE TABLE ollama_sync_jobs (
    id               UUID        PRIMARY KEY DEFAULT uuidv7(),
    started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at     TIMESTAMPTZ,
    status           TEXT        NOT NULL DEFAULT 'running',  -- 'running' | 'completed'
    total_providers  INT         NOT NULL DEFAULT 0,
    done_providers   INT         NOT NULL DEFAULT 0,
    results          JSONB       NOT NULL DEFAULT '[]'::jsonb
);
-- single init migration: 0000000001_init.sql
```

---

## Ports (application/ports/outbound/)

```rust
pub struct OllamaModel {
    pub model_name:  String,
    pub provider_id: Uuid,
    pub synced_at:   DateTime<Utc>,
}

/// Model name + count of providers that carry it (for GET /v1/ollama/models).
pub struct OllamaModelWithCount {
    pub model_name:     String,
    pub provider_count: i64,
}

/// Provider info returned by GET /v1/ollama/models/{model}/providers.
pub struct OllamaProviderForModel {
    pub provider_id: Uuid,
    pub name:        String,
    pub url:         String,
    pub status:      String,
}

pub trait OllamaModelRepository: Send + Sync {
    // Replace all models for a provider atomically (DELETE + INSERT tx)
    async fn sync_provider_models(&self, provider_id: Uuid, model_names: &[String]) -> Result<()>;
    // Distinct sorted model names across all providers (legacy — prefer list_with_counts)
    async fn list_all(&self) -> Result<Vec<String>>;
    // Distinct model names with per-model provider count: GROUP BY model_name + COUNT
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>>;
    // Provider UUIDs that have the given model (used for routing)
    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>>;
    // Provider info (id, name, url, status) that have the given model (used by UI)
    async fn providers_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaProviderForModel>>;
    // Model names synced for a specific provider (used by UI)
    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>>;
}

pub struct OllamaSyncJob {
    pub id:              Uuid,
    pub started_at:      DateTime<Utc>,
    pub completed_at:    Option<DateTime<Utc>>,
    pub status:          String,          // "running" | "completed"
    pub total_providers: i32,
    pub done_providers:  i32,
    pub results:         serde_json::Value, // JSON array (NOT NULL DEFAULT '[]')
}

pub trait OllamaSyncJobRepository: Send + Sync {
    async fn create(&self, total_providers: i32) -> Result<Uuid>;
    // Appends one result object; increments done_providers
    async fn update_progress(&self, id: Uuid, result: serde_json::Value) -> Result<()>;
    async fn complete(&self, id: Uuid) -> Result<()>;
    async fn get_latest(&self) -> Result<Option<OllamaSyncJob>>;
}
```

---

## API Endpoints (ollama_model_handlers.rs)

### Global Model Pool

```
GET /v1/ollama/models
  → { models: [{ model_name: "llama3", provider_count: 3 }, ...] }
  // distinct, sorted by model_name; provider_count = number of providers with this model
```

### Global Sync (async background)

```
POST /v1/ollama/models/sync
  1. List all active Ollama providers from registry
  2. No active providers → 400
  3. Create sync job: ollama_sync_job_repo.create(total)
  4. tokio::spawn background task (sequential, no retry):
       for each provider:
         GET {provider.url}/api/tags → parse model names
         ollama_model_repo.sync_provider_models(id, &models)
         model_selection_repo.upsert_models(id, &models)  // non-fatal; is_enabled=true for new rows
         ollama_sync_job_repo.update_progress(job_id, { provider_id, name, models, error: null })
         On failure:
           update_progress(job_id, { provider_id, name, models: [], error: "msg" })
           continue to next provider (non-fatal)
       ollama_sync_job_repo.complete(job_id)
  5. Return 202 immediately:
     → { job_id: "uuid", status: "running" }
```

### Sync Status (poll until completed)

```
GET /v1/ollama/sync/status
  → OllamaSyncJob as JSON
  → 404 if no sync has ever run

Example response (running):
{
  "id": "...", "status": "running",
  "total_providers": 3, "done_providers": 1,
  "results": [
    { "provider_id": "...", "name": "gpu-ollama-1",
      "models": ["llama3", "mistral"], "error": null }
  ]
}

Example response (completed with one failure):
{
  "id": "...", "status": "completed",
  "total_providers": 3, "done_providers": 3,
  "results": [
    { "provider_id": "...", "name": "gpu-ollama-1", "models": ["llama3"], "error": null },
    { "provider_id": "...", "name": "gpu-ollama-2", "models": [],          "error": "Connection refused" }
  ]
}
```

### Per-Model Provider Lookup

```
GET /v1/ollama/models/{model_name}/providers
  → { providers: [{ provider_id, name, url, status }, ...] }
  // all providers that have the given model synced, ordered by name
  // status: "online" | "offline" | "degraded" (live value from llm_providers)
```

### Per-Provider Model Lookup

```
GET /v1/ollama/providers/{provider_id}/models
  → { models: ["codellama", "llama3", ...] }
  // all model names synced for the given provider, sorted
```

### Per-Provider Sync (also updates DB)

`POST /v1/providers/{id}/models/sync` (handled in `backend_handlers.rs`):
1. Fetch `GET /api/tags` from the provider
2. Cache in Valkey (existing behavior, TTL 1h)
3. **Also** call `ollama_model_repo.sync_provider_models(id, &models)` → updates `ollama_models` table
4. Returns `{ models, synced: true }`

This keeps the global pool up-to-date even when only one provider is synced individually.
Also calls `model_selection_repo.upsert_models(id, &models)` (non-fatal) → inserts `is_enabled = true` for new rows, preserves existing toggle state.

---

## Model-Aware Routing (provider_router.rs)

`pick_best_provider()` accepts both `ollama_model_repo` and `model_selection_repo`.

For Ollama dispatch (two sequential filters):

```
Filter 1 — Model presence (ollama_model_repo):
  1. providers_for_model(model_name) → provider IDs that have the model synced
  2. Non-empty set → filter candidates to intersection
     Empty set (not yet synced) → use all candidates (fallback — never breaks routing)

Filter 2 — Model selection (model_selection_repo):
  3. For each candidate: list_enabled(provider_id) → Vec<String>
  4. Non-empty list + model_name NOT in list → skip this provider (disabled)
     Empty list or error → include provider (no restriction — backward compatible)

Final pick: highest available VRAM from remaining candidates
```

**Default state after sync**: all synced models are `is_enabled = true` → route normally.
Disable a model: `PATCH /v1/providers/{id}/selected-models/{model}` `{ is_enabled: false }`.

Fallbacks are intentional: new deployments with empty tables continue routing without restriction.

`InferenceUseCaseImpl` stores both repos and passes them to `pick_best_provider()` at inference time.

---

## AppState Fields (state.rs)

```rust
pub struct AppState {
    // ...
    pub ollama_model_repo:    Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo: Arc<dyn OllamaSyncJobRepository>,
}
```

Initialized in `main.rs`, wired at composition root.

---

## Web UI

→ See `docs/llm/frontend/pages/providers.md` → OllamaSyncSection
→ See `docs/llm/frontend/pages/api-test.md` → Ollama global model pool
