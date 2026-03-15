# Providers -- Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-06 (rev: automatic allocation flow)

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
  pub status: LlmProviderStatus,        // Online | Offline | Degraded
  pub registered_at: DateTime<Utc>,
}
```

## DB Schema

```sql
CREATE TABLE llm_providers (
  id                UUID         PRIMARY KEY,
  name              VARCHAR(255) NOT NULL,
  provider_type     VARCHAR(50)  NOT NULL,   -- 'ollama' | 'gemini'
  url               TEXT         NOT NULL DEFAULT '',
  api_key_encrypted TEXT,
  is_active         BOOLEAN      NOT NULL DEFAULT true,
  total_vram_mb     BIGINT       NOT NULL DEFAULT 0,
  gpu_index         SMALLINT,
  server_id         UUID REFERENCES gpu_servers(id) ON DELETE SET NULL,
  is_free_tier      BOOLEAN      NOT NULL DEFAULT false,
  status            VARCHAR(20)  NOT NULL DEFAULT 'offline',
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

---

## Automatic Ollama Allocation — End-to-End Flow

Once an Ollama provider is registered, everything works automatically: model sync, VRAM management, concurrency limits, and throughput learning.
Admins just register the provider and link a server — that's it.

### Full Lifecycle

```
1. REGISTER     POST /v1/providers {name, provider_type: "ollama", url}
                → health check → status: online/offline
                → POST /v1/servers {name, node_exporter_url}
                → PATCH /v1/providers/{id} {server_id, gpu_index}

2. AUTO SYNC    Background sync loop (30s tick, 300s cooldown)
                → /api/version (health) → /api/tags (models) → /api/ps (loaded)
                → /api/show (architecture) → throughput stats → KV compute
                → AIMD update → LLM batch analysis

3. REQUEST      POST /v1/chat/completions {model: "qwen3:8b", ...}
                → provider selection → VRAM gate → concurrency gate → dispatch

4. LEARN        Completed job → throughput recorded → next sync uses for AIMD
                → 3+ samples: AIMD adjusts max_concurrent
                → 10+ samples: LLM batch recommends optimal allocation

5. RESTART      Server restart → restore learned data from DB → apply immediately
```

### Phase 1: Provider Registration → Automatic Model Discovery

```
POST /v1/providers {name: "gpu-server", provider_type: "ollama", url: "https://ollama.example.com"}
  │
  ├── health check: GET {url}/api/version
  │   → online: status = "online", model sync available
  │   → offline: status = "offline", sync skipped
  │
  ├── model sync: GET {url}/api/tags
  │   → saved to ollama_models table (per provider)
  │   → registered in provider_selected_models with default is_enabled=true
  │   → Valkey cache: veronex:models:{provider_id} (TTL 30s)
  │
  └── server link (optional):
      POST /v1/servers {name, node_exporter_url}
      PATCH /v1/providers/{id} {server_id, gpu_index: 0}
      → enables GPU VRAM and temperature collection from node-exporter
```

### Phase 2: Request → Provider Selection → Allocation

```
POST /v1/chat/completions {model: "qwen3:8b", messages: [...]}
  │
  ├── 1. API Key auth → verify account_id, tier (free/paid)
  │
  ├── 2. Enqueue in Valkey (tier-based priority)
  │     paid → veronex:queue:jobs:paid   (highest priority)
  │     free → veronex:queue:jobs        (standard)
  │     test → veronex:queue:jobs:test   (lowest priority)
  │
  ├── 3. queue_dispatcher_loop pops via Lua priority pop
  │
  ├── 4. Provider selection (pick_best_provider)
  │     a. List active Ollama providers
  │     b. Model filter: only providers that have the model in ollama_models
  │     c. Selection filter: only enabled entries in provider_selected_models
  │     d. VRAM sort: highest available VRAM first (most headroom among servers)
  │     e. Tier sort: paid key → non-free-tier first, free key → free-tier first
  │
  ├── 5. Gate checks (in order)
  │     a. Circuit Breaker: skip providers with consecutive failures
  │     b. Thermal: ≥85°C Soft (skip if active>0), ≥92°C Hard (fully blocked)
  │     c. Concurrency: block if exceeds max_concurrent (cold start=1)
  │     d. VRAM: vram_pool.try_reserve() → reserve KV cache + (weight if needed)
  │
  ├── 6. Dispatch → Ollama API
  │     OllamaAdapter: POST {url}/api/chat (streaming)
  │     If model not loaded, Ollama auto-loads (weight stays in VRAM)
  │
  └── 7. Completion → Cleanup
        Drop(VramPermit) → release KV cache, active_count -= 1
        circuit_breaker.on_success/on_failure
        Save result to inference_jobs table
```

### Phase 3: Automatic Learning — Cold Start → AIMD → LLM Batch

```
                     ┌─────────────────────────────────────────────────┐
                     │          Sync Loop (30s tick)                   │
                     │                                                 │
  ┌──────────┐       │  ┌─────────────┐   ┌─────────┐   ┌──────────┐ │
  │ Provider  │──────▶│  │ Cold Start  │──▶│  AIMD   │──▶│ LLM Batch│ │
  │ Register  │       │  │ limit = 1   │   │ ±adjust │   │ optimal  │ │
  │           │       │  │ (all models)│   │(per-model)│  │(all combos)│ │
  └──────────┘       │  └──────┬──────┘   └────┬────┘   └─────┬────┘ │
                     │         │               │              │       │
                     │    sample=0         sample≥3       sample≥10   │
                     │    baseline=0       ratio based    LLM analysis │
                     │                                                 │
                     │  ┌──────────────────────────────────────────┐   │
                     │  │ DB persist: model_vram_profiles          │   │
                     │  │  max_concurrent, baseline_tps            │   │
                     │  │  → auto-restored on server restart       │   │
                     │  └──────────────────────────────────────────┘   │
                     └─────────────────────────────────────────────────┘
```

| Phase | Condition | max_concurrent | Behavior |
|-------|-----------|---------------|----------|
| **Cold Start** | New model, no data | 1 | 1 request per model. Collect baseline |
| **AIMD** | sample ≥ 3, baseline exists | Auto-adjusted | ratio ≥ 0.9 → +1, < 0.7 → ×3/4 |
| **LLM Batch** | total sample ≥ 10 | LLM recommended | All model combinations + VRAM + throughput analysis |

### Phase 4: Multi-Server / Multi-Model Automatic Routing

Registering multiple Ollama servers enables automatic routing to the optimal server.

```
Example: 3 servers, various models

Server A (128GB GPU)                    Server B (24GB GPU)          Server C (CPU only)
├── qwen3:72b (40GB)    limit=2        ├── qwen3:8b (5GB)  limit=4  ├── qwen3:1.7b  limit=3
├── deepseek-r1:70b (45GB) limit=1     └── phi4:14b (9GB)  limit=3  └── phi4-mini   limit=5
└── available: 35GB                        available: 8GB

Request: model=qwen3:8b
  → Server B selected (has model + VRAM headroom)
  → limit=4, active=2 → allowed

Request: model=deepseek-r1:70b
  → Server A selected (only server with model)
  → limit=1, active=1 → queued (cold start or AIMD limit)

Request: model=qwen3:1.7b
  → Server C selected (has model)
  → VRAM=0 (CPU) → delegated to Ollama, only concurrency gate applied
```

**Routing priority**:
1. Only providers that have the requested model are candidates
2. Only providers with model enabled in model selection
3. Prefer providers with more available VRAM
4. On equal VRAM, paid tier key → non-free-tier provider first
5. Must pass Thermal/Circuit Breaker gates

### Phase 5: Adding a New Model

When a new model is pulled on Ollama, it is auto-detected on the next sync.

```
ollama pull llama3.3:70b  (directly on the Ollama server)
  │
  ├── Next sync (≤300s)
  │   GET /api/tags → new model discovered
  │   → auto-added to ollama_models table
  │   → registered in provider_selected_models with is_enabled=true
  │
  ├── First request arrives
  │   → try_reserve: max_concurrent=1 (cold start, no learned data)
  │   → Ollama auto-loads the model → weight occupies VRAM
  │
  ├── First sync with loaded model
  │   → weight measured from /api/ps → saved to model_vram_profiles
  │   → architecture parsed from /api/show → KV cache calculated
  │   → baseline_tps set (first throughput data)
  │
  └── Subsequent automatic learning
      → AIMD: auto-adjusts from sample ≥ 3
      → LLM Batch: full model combination analysis from total sample ≥ 10
```

**Cases requiring manual intervention**:
- Disable a specific model on a specific provider: `PATCH /v1/providers/{id}/selected-models/{model} {is_enabled: false}`
- Change probe policy: `PATCH /v1/dashboard/capacity/settings {probe_permits, probe_rate}`
- Trigger immediate sync: `POST /v1/providers/sync`

### Configuration Reference

| Setting | Default | Location | Description |
|---------|---------|----------|-------------|
| sync_interval_secs | 300 | capacity_settings | Auto sync interval |
| sync_enabled | true | capacity_settings | Auto sync ON/OFF |
| analyzer_model | qwen2.5:3b | capacity_settings | Model for LLM analysis |
| probe_permits | 1 | capacity_settings | +N (probe up), -N (probe down), 0=disabled |
| probe_rate | 3 | capacity_settings | 1 probe per N limit hits |
| CAPACITY_ANALYZER_OLLAMA_URL | (provider URL) | env | LLM analysis target (can be separate) |

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
