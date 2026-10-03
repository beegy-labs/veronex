# Providers — llama-server: Modelfile Registry & Model-Aware Routing

> SSOT | **Last Updated**: 2026-05-10

The legacy "global Ollama model pool" (`ollama_models` + `ollama_sync_jobs`
tables, `POST /v1/ollama/models/sync` handler) was removed when veronex
migrated to llama-server. Veronex no longer scrapes per-provider
`/api/tags`; instead, models are catalogued centrally in the **Modelfile
Registry** (Phase 2) and providers reference rows from it.

## Task Guide

| Task | Endpoint / File | Notes |
|------|-----------------|-------|
| List all registered models | `GET /v1/admin/models` | `admin_modelfile_handlers::list_models` |
| Register a new model (Modelfile) | `POST /v1/admin/models` | Body: model spec (HF source, S3 pointer, etc.) |
| Get model detail | `GET /v1/admin/models/{id}` | |
| Patch model metadata | `PATCH /v1/admin/models/{id}` | |
| Delete a model (registry only) | `DELETE /v1/admin/models/{id}` | |
| Promote a draft model to ready | `POST /v1/admin/models/{id}/promote` | Atomic flag flip |
| List install attempts | `GET /v1/admin/models/{id}/install/attempts` | History of resolve → download |
| Retry a failed install | `POST /v1/admin/models/{id}/install/retry` | Idempotent |
| Cancel an in-flight install | `POST /v1/admin/models/{id}/install/cancel` | |
| Stream install progress (SSE) | `GET /v1/admin/models/{id}/install/stream` | `install_orchestrator` events |
| List orphan CAS blobs | `GET /v1/admin/blobs` | Operator-driven GC |
| Delete an orphan blob | `DELETE /v1/admin/blobs/{sha256}` | |
| OpenAI-compat model list | `GET /v1/models` | `openai_models_handlers::list_models` — every Modelfile |
| Toggle a provider/model enable flag | `PATCH /v1/providers/{id}/selected-models/{model}` | `model_selection_handlers::set_model_enabled` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/admin_modelfile_handlers.rs` | All `/v1/admin/models/*` + `/v1/admin/blobs` handlers |
| `crates/veronex/src/application/ports/outbound/modelfile_registry.rs` | `ModelfileRegistry` trait (registry CRUD) |
| `crates/veronex/src/infrastructure/outbound/persistence/modelfile_registry.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/outbound/model_store/install_orchestrator.rs` | Resolves source → CAS blob, drives install state machine, emits SSE events |
| `crates/veronex/src/infrastructure/outbound/model_store/local_pv.rs` | CAS blob persistence (`{root}/blobs/sha256/{prefix}/{sha}`) |
| `crates/veronex/src/infrastructure/outbound/model_store/source/` | `hf_source` + `s3_pointer_source` resolvers |
| `crates/veronex/src/infrastructure/inbound/http/openai_models_handlers.rs` | `GET /v1/models` + `GET /v1/models/{id}` (OpenAI compat) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `pick_best_provider()` — model-aware filter (uses Modelfile rows) |

## DB Schema (current)

| Table | Role |
|-------|------|
| `modelfiles` | Central catalogue — one row per registered model. Owned by registry. |
| `model_install_attempts` | Append-only log of resolve/download attempts per modelfile. |
| `provider_selected_models` | Per-provider opt-in flags pointing at `modelfiles.id`. |

The legacy `ollama_models` and `ollama_sync_jobs` tables and their indexes
were dropped in the post-Ollama migration (`docker/postgres/init.sql`).

## Routing

`provider_router::pick_best_provider()` keeps the same model-aware filter
shape it had in the Ollama era — only the source of truth changed:
candidates are filtered to providers whose `provider_selected_models`
row references the requested model AND has `is_enabled = true`.

Cross-references:
- Install state machine, CAS layout, install SSE → `infra/model-store.md` (if present) or `model_store/install_orchestrator.rs`
- AIMD admission + ProcessManager spawn flow → `flows/process-manager.md`
- Setup wizard for first-run model registration → `frontend/pages/setup.md`
