# Backends — Ollama: Global Model Sync & Model-Aware Routing

> SSOT | **Last Updated**: 2026-03-02 (Ollama model enable/disable — upsert to backend_selected_models on sync; selection filter in router)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Trigger global sync of all Ollama backends | `POST /v1/ollama/models/sync` | `ollama_model_handlers.rs` → `sync_all_backends()` |
| Poll sync progress | `GET /v1/ollama/sync/status` | `ollama_model_handlers.rs` → `get_sync_status()` |
| Get global model pool (with counts) | `GET /v1/ollama/models` | `ollama_model_handlers.rs` → `list_models()` → `list_with_counts()` |
| Get backends for a model | `GET /v1/ollama/models/{model}/backends` | `ollama_model_handlers.rs` → `list_model_backends()` |
| Get models for a backend | `GET /v1/ollama/backends/{id}/models` | `ollama_model_handlers.rs` → `list_backend_models()` |
| Per-backend sync (also updates DB) | `POST /v1/backends/{id}/models/sync` | `backend_handlers.rs` → `sync_backend_models()` (Ollama path) |
| Change model-aware routing filter | `infrastructure/outbound/backend_router.rs` → `pick_best_backend()` | Modify `ollama_model_repo` filter block |
| Toggle a model on/off for a backend | `PATCH /v1/backends/{id}/selected-models/{model}` | `set_model_enabled()` in `backend_handlers.rs` → `backend_selected_models` |
| Change model selection default on sync | `backend_handlers.rs` → `sync_backend_models()` + `ollama_model_handlers.rs` → `sync_all_backends()` | `upsert_models()` inserts `is_enabled = true` for new rows |
| Change modal pagination size | `web/app/providers/page.tsx` `BACKENDS_PAGE_SIZE` | Affects `OllamaModelBackendsModal` |
| Add field to OllamaModel | `migrations/` + `ollama_model_repository.rs` (port + pg impl) | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/infrastructure/inbound/http/ollama_model_handlers.rs` | Global sync + status + model list handlers |
| `crates/inferq/src/application/ports/outbound/ollama_model_repository.rs` | `OllamaModelRepository` trait |
| `crates/inferq/src/application/ports/outbound/ollama_sync_job_repository.rs` | `OllamaSyncJobRepository` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/ollama_model_repository.rs` | Postgres impl |
| `crates/inferq/src/infrastructure/outbound/persistence/ollama_sync_job_repository.rs` | Postgres impl |
| `crates/inferq/src/infrastructure/inbound/http/backend_handlers.rs` | `sync_backend_models` (Ollama path) — persists to `ollama_models` |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `pick_best_backend()` — model-aware Ollama candidate filter |
| `crates/inferq/src/infrastructure/inbound/http/state.rs` | `AppState` fields |

---

## DB Schema

```sql
-- Per-backend model list (primary key on pair)
CREATE TABLE ollama_models (
    model_name TEXT NOT NULL,
    backend_id UUID NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (model_name, backend_id)
);

-- Global sync job tracking (persists progress across page navigation)
CREATE TABLE ollama_sync_jobs (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at   TIMESTAMPTZ,
    status         TEXT        NOT NULL DEFAULT 'running',  -- 'running' | 'completed'
    total_backends INT         NOT NULL DEFAULT 0,
    done_backends  INT         NOT NULL DEFAULT 0,
    results        JSONB       NOT NULL DEFAULT '[]'::jsonb
);
-- migrations: 000026 ollama_models, 000027 ollama_sync_jobs
```

---

## Ports (application/ports/outbound/)

```rust
pub struct OllamaModel {
    pub model_name: String,
    pub backend_id: Uuid,
    pub synced_at:  DateTime<Utc>,
}

/// Model name + count of backends that carry it (for GET /v1/ollama/models).
pub struct OllamaModelWithCount {
    pub model_name:    String,
    pub backend_count: i64,
}

/// Backend info returned by GET /v1/ollama/models/{model}/backends.
pub struct OllamaBackendForModel {
    pub backend_id: Uuid,
    pub name:       String,
    pub url:        String,
    pub status:     String,
}

pub trait OllamaModelRepository: Send + Sync {
    // Replace all models for a backend atomically (DELETE + INSERT tx)
    async fn sync_backend_models(&self, backend_id: Uuid, model_names: &[String]) -> Result<()>;
    // Distinct sorted model names across all backends (legacy — prefer list_with_counts)
    async fn list_all(&self) -> Result<Vec<String>>;
    // Distinct model names with per-model backend count: GROUP BY model_name + COUNT
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>>;
    // Backend UUIDs that have the given model (used for routing)
    async fn backends_for_model(&self, model_name: &str) -> Result<Vec<Uuid>>;
    // Backend info (id, name, url, status) that have the given model (used by UI)
    async fn backends_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaBackendForModel>>;
    // Model names synced for a specific backend (used by UI)
    async fn models_for_backend(&self, backend_id: Uuid) -> Result<Vec<String>>;
}

pub struct OllamaSyncJob {
    pub id:             Uuid,
    pub started_at:     DateTime<Utc>,
    pub completed_at:   Option<DateTime<Utc>>,
    pub status:         String,          // "running" | "completed"
    pub total_backends: i32,
    pub done_backends:  i32,
    pub results:        serde_json::Value, // JSON array (NOT NULL DEFAULT '[]')
}

pub trait OllamaSyncJobRepository: Send + Sync {
    async fn create(&self, total_backends: i32) -> Result<Uuid>;
    // Appends one result object; increments done_backends
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
  → { models: [{ model_name: "llama3", backend_count: 3 }, ...] }
  // distinct, sorted by model_name; backend_count = number of backends with this model
```

### Global Sync (async background)

```
POST /v1/ollama/models/sync
  1. List all active Ollama backends from registry
  2. No active backends → 400
  3. Create sync job: ollama_sync_job_repo.create(total)
  4. tokio::spawn background task (sequential, no retry):
       for each backend:
         GET {backend.url}/api/tags → parse model names
         ollama_model_repo.sync_backend_models(id, &models)
         model_selection_repo.upsert_models(id, &models)  // non-fatal; is_enabled=true for new rows
         ollama_sync_job_repo.update_progress(job_id, { backend_id, name, models, error: null })
         On failure:
           update_progress(job_id, { backend_id, name, models: [], error: "msg" })
           continue to next backend (non-fatal)
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
  "total_backends": 3, "done_backends": 1,
  "results": [
    { "backend_id": "...", "name": "gpu-ollama-1",
      "models": ["llama3", "mistral"], "error": null }
  ]
}

Example response (completed with one failure):
{
  "id": "...", "status": "completed",
  "total_backends": 3, "done_backends": 3,
  "results": [
    { "backend_id": "...", "name": "gpu-ollama-1", "models": ["llama3"], "error": null },
    { "backend_id": "...", "name": "gpu-ollama-2", "models": [],          "error": "Connection refused" }
  ]
}
```

### Per-Model Backend Lookup

```
GET /v1/ollama/models/{model_name}/backends
  → { backends: [{ backend_id, name, url, status }, ...] }
  // all backends that have the given model synced, ordered by name
  // status: "online" | "offline" | "degraded" (live value from llm_backends)
```

### Per-Backend Model Lookup

```
GET /v1/ollama/backends/{backend_id}/models
  → { models: ["codellama", "llama3", ...] }
  // all model names synced for the given backend, sorted
```

### Per-Backend Sync (also updates DB)

`POST /v1/backends/{id}/models/sync` (handled in `backend_handlers.rs`):
1. Fetch `GET /api/tags` from the backend
2. Cache in Valkey (existing behavior, TTL 1h)
3. **Also** call `ollama_model_repo.sync_backend_models(id, &models)` → updates `ollama_models` table
4. Returns `{ models, synced: true }`

This keeps the global pool up-to-date even when only one backend is synced individually.
Also calls `model_selection_repo.upsert_models(id, &models)` (non-fatal) → inserts `is_enabled = true` for new rows, preserves existing toggle state.

---

## Model-Aware Routing (backend_router.rs)

`pick_best_backend()` accepts both `ollama_model_repo` and `model_selection_repo`.

For Ollama dispatch (two sequential filters):

```
Filter 1 — Model presence (ollama_model_repo):
  1. backends_for_model(model_name) → backend IDs that have the model synced
  2. Non-empty set → filter candidates to intersection
     Empty set (not yet synced) → use all candidates (fallback — never breaks routing)

Filter 2 — Model selection (model_selection_repo):
  3. For each candidate: list_enabled(backend_id) → Vec<String>
  4. Non-empty list + model_name NOT in list → skip this backend (disabled)
     Empty list or error → include backend (no restriction — backward compatible)

Final pick: highest available VRAM from remaining candidates
```

**Default state after sync**: all synced models are `is_enabled = true` → route normally.
Disable a model: `PATCH /v1/backends/{id}/selected-models/{model}` `{ is_enabled: false }`.

Fallbacks are intentional: new deployments with empty tables continue routing without restriction.

`InferenceUseCaseImpl` stores both repos and passes them to `pick_best_backend()` at inference time.

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

→ See `docs/llm/frontend/web-backends.md` → OllamaSyncSection
→ See `docs/llm/frontend/web-test.md` → Ollama global model pool
