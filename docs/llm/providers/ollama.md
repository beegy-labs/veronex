# Providers -- Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-28 (rev: num_parallel, varchar widths)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to provider API request/response | `provider_handlers.rs` -- `RegisterProviderRequest` / `UpdateProviderRequest` + migration |
| Change VRAM dispatch algorithm | `infrastructure/outbound/provider_router.rs` -- `dispatch()` function |
| Change health check logic | `infrastructure/outbound/health_checker.rs` -- `check_provider()` |
| Add new model management endpoint | `provider_handlers.rs` + `router.rs` |
| Change VRAM pool logic | `infrastructure/outbound/capacity/vram_pool.rs` -- `VramPool` |
| Change thermal throttle thresholds | `infrastructure/outbound/capacity/thermal.rs` -- `ThermalThrottleMap::update()` |
| Add new LlmProvider DB column | `migrations/` new file + `domain/entities/mod.rs` + `persistence/provider_registry.rs` |
| Change provider list cache TTL | `persistence/caching_provider_registry.rs` -- `CachingProviderRegistry::new()` TTL arg in `main.rs` |
| Toggle a model on/off per Ollama provider | `PATCH /v1/providers/{id}/selected-models/{model}` -- `set_model_enabled()` in `provider_handlers.rs` |
| Change Ollama model selection defaults | `provider_handlers.rs` -- `list_selected_models()` Ollama branch -- default is `true` |
| Change streaming/context behavior | See `docs/llm/providers/ollama-impl.md` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/mod.rs` | `LlmProvider` entity |
| `crates/veronex/src/application/ports/outbound/` | `LlmProviderRegistry` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/provider_registry.rs` | `PostgresProviderRegistry` (DB adapter) |
| `crates/veronex/src/infrastructure/outbound/persistence/caching_provider_registry.rs` | `CachingProviderRegistry` (5s TTL cache decorator) |
| `crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` | `OllamaAdapter` (streaming) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `DynamicProviderRouter` + `queue_dispatcher_loop` |
| `crates/veronex/src/infrastructure/outbound/health_checker.rs` | 30s background health checker |
| `crates/veronex/src/infrastructure/inbound/http/provider_handlers.rs` | CRUD + model management handlers |

---

## LlmProvider Entity

```rust
// domain/entities/mod.rs
pub struct LlmProvider {
  pub id: Uuid,
  pub name: String,
  pub provider_type: ProviderType,       // Ollama | Gemini
  pub url: String,                       // "http://host:11434" (Ollama) | "" (Gemini)
  pub api_key_encrypted: Option<String>,
  pub is_active: bool,
  pub total_vram_mb: i64,               // 0 = unlimited
  pub gpu_index: Option<i16>,           // 0-based GPU index on host
  pub server_id: Option<Uuid>,          // FK -> gpu_servers (Gemini = NULL)
  pub is_free_tier: bool,               // Gemini only
  pub num_parallel: i16,                // Ollama num_parallel (default 4)
  pub status: LlmProviderStatus,        // Online | Offline | Degraded
  pub registered_at: DateTime<Utc>,
}
```

## DB Schema

```sql
CREATE TABLE llm_providers (
  id                UUID         PRIMARY KEY,
  name              VARCHAR(255) NOT NULL,
  provider_type     VARCHAR(32)  NOT NULL,   -- 'ollama' | 'gemini'
  url               TEXT         NOT NULL DEFAULT '',
  api_key_encrypted TEXT,
  is_active         BOOLEAN      NOT NULL DEFAULT true,
  total_vram_mb     BIGINT       NOT NULL DEFAULT 0,
  gpu_index         SMALLINT,
  server_id         UUID REFERENCES gpu_servers(id) ON DELETE SET NULL,
  is_free_tier      BOOLEAN      NOT NULL DEFAULT false,
  num_parallel      SMALLINT     NOT NULL DEFAULT 4,
  status            VARCHAR(32)  NOT NULL DEFAULT 'offline',
  registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- single init migration: 0000000001_init.sql
```

---

## API Endpoints (provider_handlers.rs)

```
POST   /v1/providers                   RegisterProviderRequest -> RegisterProviderResponse
GET    /v1/providers                   -> Vec<ProviderSummary>
PATCH  /v1/providers/{id}             UpdateProviderRequest -> 200
DELETE /v1/providers/{id}             -> 204
POST   /v1/providers/{id}/sync          -> { status, models_synced, vram_updated }
       Unified: health check + model sync + VRAM probe (Ollama only)

POST   /v1/providers/sync               -> 202 { synced_count }
       Triggers sync for all Ollama providers

GET    /v1/providers/{id}/models
       Ollama -> GET /api/tags (live)
       Gemini -> 400 "Use GET /v1/gemini/models"

GET    /v1/providers/{id}/key         -> { api_key } (decrypted, admin only)

GET    /v1/providers/{id}/selected-models
       Ollama -> per-provider list (ollama_models) merged with provider_selected_models
               default is_enabled = true for rows not yet in selection table
       Gemini -> global gemini_models merged with provider_selected_models (default false)

PATCH  /v1/providers/{id}/selected-models/{model_name}
       { is_enabled: bool } -> 204   (shared handler, same table)
```

### Global Model Pool (ollama_model_handlers.rs)

```
GET  /v1/ollama/models         -> { models: ["llama3", "mistral", ...] }  // distinct, sorted
POST /v1/ollama/models/sync    -> 202 { job_id, status: "running" }       // async, no retry
GET  /v1/ollama/sync/status    -> OllamaSyncJob (progress + per-provider results)
```

See `docs/llm/providers/ollama-models.md` for full spec.

### Request Structs

```rust
pub struct RegisterProviderRequest {
  pub name: String,
  pub provider_type: ProviderType,
  pub url: Option<String>,
  pub api_key: Option<String>,
  pub total_vram_mb: Option<i64>,
  pub gpu_index: Option<i16>,
  pub server_id: Option<Uuid>,
  pub is_free_tier: Option<bool>,
}

pub struct UpdateProviderRequest {
  pub name: String,
  pub url: Option<String>,
  pub api_key: Option<String>,          // "" or null -> keeps existing key
  pub total_vram_mb: Option<i64>,
  pub gpu_index: Option<Option<i16>>,   // Some(None) -> clears FK
  pub server_id: Option<Option<Uuid>>,  // Some(None) -> clears FK
  pub is_active: Option<bool>,
  pub is_free_tier: Option<bool>,
}
```

SQL for PATCH: `COALESCE($3, api_key_encrypted)` preserves existing key when `api_key = ""`.

---

## Provider Registry Caching

`CachingProviderRegistry` wraps `PostgresProviderRegistry` with a 5-second TTL in-memory cache. This prevents hundreds of Postgres queries/second from `queue_dispatcher_loop` calling `list_all()` on every job dequeue.

- **`list_all()`**: shared read lock fast path; write lock + DB query on miss. Double-checked locking prevents thundering herd.
- **Mutating methods** (`register`, `update_status`, `update`, `deactivate`): forward to inner + invalidate cache.
- **Read-only methods** (`list_active`, `get`): forward directly (called infrequently, no cache needed).

→ Automatic allocation flow: `ollama-allocation.md`

---

## Background Loops

### Sync Loop (run_sync_loop — analyzer.rs)
- Tick: 30s, Cooldown: `capacity_settings.sync_interval_secs` (default 300s)
- Manual trigger: `POST /v1/providers/sync` (ignores cooldown)
- Per Ollama provider:
  1. `/api/version` → health check
  2. `/api/tags` → model sync (DB + Valkey cache)
  3. `/api/ps` → loaded model weight measurement
  4. `/api/show` → architecture parsing (hybrid Mamba+Attention support)
  5. throughput stats (PG) → KV per request calculation
  6. AIMD → max_concurrent adjustment
  7. LLM batch → all-model combination analysis (sample ≥ 10)
  8. DB persist → model_vram_profiles
- Gemini: not included (no VRAM concept)

### Health Checker (health_checker.rs)
- Interval: 30 seconds
- Ollama only: `GET {url}/api/version` (timeout: `OLLAMA_HEALTH_CHECK_TIMEOUT` = 5s) → 200 (background auto-check)
- Gemini: **not auto-checked** — `GET /v1beta/models?pageSize=1` + `x-goog-api-key` header, called only on manual sync or per-row healthcheck button
- After hw_metrics load: `thermal.update(provider_id, temp_c)` → Normal/Soft/Hard
  - Sets/removes `veronex:throttle:{provider_id}` in Valkey (TTL 360s)

---

## Related Documents

- **VRAM pool + thermal + AIMD details**: `docs/llm/inference/capacity.md`
- **Streaming protocol + format conversion**: `docs/llm/providers/ollama-impl.md`
- **Ollama model sync**: `docs/llm/providers/ollama-models.md`
- **Web UI**: `docs/llm/frontend/pages/providers.md` -- OllamaTab + OllamaSyncSection
