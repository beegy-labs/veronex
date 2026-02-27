# Backends — Gemini: Global Model Sync & Per-Backend Selection

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Set admin API key | `PUT /v1/gemini/sync-config` | `gemini_model_handlers.rs` → `set_sync_config()` |
| Sync global model list | `POST /v1/gemini/models/sync` | `gemini_model_handlers.rs` → `sync_models()` |
| Change sync behavior (filter logic) | `infrastructure/outbound/gemini/adapter.rs` | model list filter in sync handler |
| Enable/disable model for paid backend | `PATCH /v1/backends/{id}/selected-models/{model}` | `backend_handlers.rs` → `set_model_enabled()` |
| Change merge logic (global + per-backend) | `backend_handlers.rs` → `list_selected_models()` | `gemini_model_repo.list()` + `sel_map` merge |
| Add field to BackendSelectedModel | `migrations/` + `domain/entities/` + `persistence/backend_model_selection.rs` | |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/infrastructure/inbound/http/gemini_model_handlers.rs` | Sync config + model sync handlers |
| `crates/inferq/src/application/ports/outbound/gemini_sync_config_repository.rs` | `GeminiSyncConfigRepository` trait |
| `crates/inferq/src/application/ports/outbound/gemini_model_repository.rs` | `GeminiModelRepository` trait |
| `crates/inferq/src/application/ports/outbound/backend_model_selection.rs` | `BackendModelSelectionRepository` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/gemini_sync_config.rs` | Postgres impl |
| `crates/inferq/src/infrastructure/outbound/persistence/gemini_model_repository.rs` | Postgres impl |
| `crates/inferq/src/infrastructure/outbound/persistence/backend_model_selection.rs` | Postgres impl (UPSERT) |
| `crates/inferq/src/infrastructure/inbound/http/backend_handlers.rs` | `list_selected_models`, `set_model_enabled` |
| `crates/inferq/src/infrastructure/inbound/http/state.rs` | `AppState` fields |

---

## DB Schema

```sql
-- Global admin API key (singleton, id always = 1)
CREATE TABLE gemini_sync_config (
    id                INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    api_key_encrypted TEXT    NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Global Gemini model pool (synced via admin key)
CREATE TABLE gemini_models (
    model_name TEXT        PRIMARY KEY,
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per paid-backend model filter
CREATE TABLE backend_selected_models (
    backend_id  UUID         NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    model_name  VARCHAR(255) NOT NULL,
    is_enabled  BOOLEAN      NOT NULL DEFAULT false,
    added_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (backend_id, model_name)
);
-- migrations: 000022 backend_selected_models, 000023 gemini_sync_config, 000024 gemini_models
```

---

## Ports (application/ports/outbound/)

```rust
pub trait GeminiSyncConfigRepository: Send + Sync {
    async fn get_api_key(&self) -> Result<Option<String>>;  // decrypted
    async fn set_api_key(&self, api_key: &str) -> Result<()>; // UPSERT id=1
}

pub struct GeminiModel { pub model_name: String, pub synced_at: DateTime<Utc> }
pub trait GeminiModelRepository: Send + Sync {
    async fn sync_models(&self, model_names: &[String]) -> Result<()>; // DELETE + INSERT tx
    async fn list(&self) -> Result<Vec<GeminiModel>>;
}

pub trait BackendModelSelectionRepository: Send + Sync {
    async fn list(&self, backend_id: Uuid) -> Result<Vec<BackendSelectedModel>>;
    async fn list_enabled(&self, backend_id: Uuid) -> Result<Vec<String>>;
    async fn set_enabled(&self, backend_id: Uuid, model_name: &str, enabled: bool) -> Result<()>; // UPSERT
}
```

---

## API Endpoints

### Gemini Sync Config (gemini_model_handlers.rs)

```
GET  /v1/gemini/sync-config    → { api_key_masked: "AIza...xyz" | null }
PUT  /v1/gemini/sync-config    { api_key: String } → 204
```

### Global Model Sync (gemini_model_handlers.rs)

```
POST /v1/gemini/models/sync
  1. get_api_key() → None → 400
  2. Call Gemini v1beta/models?key=KEY → filter by generateContent
  3. sync_models(&names) → DELETE + INSERT transaction
  → { models: Vec<String>, count: usize }

GET  /v1/gemini/models
  → { models: [{ model_name: String, synced_at: String }] }
```

### Per-Backend Model Selection (backend_handlers.rs)

```
GET   /v1/backends/{id}/selected-models
  1. gemini_model_repo.list() → global pool
  2. model_selection_repo.list(id) → per-backend selections
  3. Merge: is_enabled = selections_map.get(name).unwrap_or(false)
  → { models: [{ model_name, is_enabled, synced_at }] }

PATCH /v1/backends/{id}/selected-models/{model_name}
  { is_enabled: bool } → 200
  Uses UPSERT: INSERT … ON CONFLICT(backend_id, model_name) DO UPDATE SET is_enabled=$3
```

---

## Router Filtering

`pick_gemini_backend()` in `backend_router.rs`:
- For paid backends (`is_free_tier=false`): call `list_enabled(backend_id)`
- If backend has any enabled models AND requested model NOT in list → skip that backend
- Backend with no entries in `backend_selected_models` → accepts all models

---

## AppState Fields (state.rs)

```rust
pub struct AppState {
    // ...
    pub gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo:        Arc<dyn GeminiModelRepository>,
    pub model_selection_repo:     Arc<dyn BackendModelSelectionRepository>,
}
```

Initialized in `main.rs`, wired at composition root.

---

## Web UI

→ See `docs/llm/frontend/web-backends.md` → GeminiSyncSection + ModelSelectionModal
