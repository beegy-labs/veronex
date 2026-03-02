# Providers — Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-03 (rev: Ollama model enable/disable — `provider_selected_models` per-provider toggle, selection filter in router)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to provider API request/response | `backend_handlers.rs` → `RegisterProviderRequest` / `UpdateProviderRequest` + migration |
| Change VRAM dispatch algorithm | `infrastructure/outbound/provider_router.rs` → `dispatch()` function |
| Change health check logic | `infrastructure/outbound/health_checker.rs` → `check_provider()` |
| Add new model management endpoint | `backend_handlers.rs` + `router.rs` |
| Change concurrency slot allocation | `infrastructure/outbound/capacity/slot_map.rs` → `ConcurrencySlotMap::update_capacity()` |
| Change thermal throttle thresholds | `infrastructure/outbound/capacity/thermal.rs` → `ThermalThrottleMap::update()` |
| Add new LlmProvider DB column | `migrations/` new file + `domain/entities/llm_provider.rs` + `persistence/provider_registry.rs` |
| Change provider list cache TTL | `persistence/caching_provider_registry.rs` → `CachingProviderRegistry::new()` TTL arg in `main.rs` |
| Toggle a model on/off per Ollama provider | `PATCH /v1/providers/{id}/selected-models/{model}` → `set_model_enabled()` in `backend_handlers.rs` |
| Change Ollama model selection defaults | `backend_handlers.rs` → `list_selected_models()` Ollama branch — default is `true` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/llm_provider.rs` | `LlmProvider` entity |
| `crates/veronex/src/application/ports/outbound/` | `LlmProviderRegistry` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/provider_registry.rs` | `PostgresProviderRegistry` (DB adapter) |
| `crates/veronex/src/infrastructure/outbound/persistence/caching_provider_registry.rs` | `CachingProviderRegistry` (5s TTL cache decorator) |
| `crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` | `OllamaAdapter` (streaming) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `DynamicProviderRouter` + `queue_dispatcher_loop` |
| `crates/veronex/src/infrastructure/outbound/health_checker.rs` | 30s background health checker |
| `crates/veronex/src/infrastructure/inbound/http/backend_handlers.rs` | CRUD + model management handlers |

---

## LlmProvider Entity

```rust
// domain/entities/llm_provider.rs
pub struct LlmProvider {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,       // Ollama | Gemini
    pub url: String,                       // "http://host:11434" (Ollama) | "" (Gemini)
    pub api_key_encrypted: Option<String>,
    pub is_active: bool,
    pub total_vram_mb: i64,               // 0 = unlimited
    pub gpu_index: Option<i16>,           // 0-based GPU index on host
    pub server_id: Option<Uuid>,          // FK → gpu_servers (Gemini = NULL)
    pub agent_url: Option<String>,        // Phase 2 sidecar (unused)
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
    agent_url         TEXT,
    is_free_tier      BOOLEAN      NOT NULL DEFAULT false,
    status            VARCHAR(20)  NOT NULL DEFAULT 'offline',
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- single init migration: 0000000001_init.sql
```

---

## API Endpoints (backend_handlers.rs)

```
POST   /v1/providers                   RegisterProviderRequest → RegisterProviderResponse
GET    /v1/providers                   → Vec<ProviderSummary>
PATCH  /v1/providers/{id}             UpdateProviderRequest → 200
DELETE /v1/providers/{id}             → 204
POST   /v1/providers/{id}/healthcheck → { status: "online" | "offline" | "degraded" }

GET    /v1/providers/{id}/models
       Ollama → GET /api/tags (live)
       Gemini → 400 "Use GET /v1/gemini/models"
       → { models: Vec<String> }

POST   /v1/providers/{id}/models/sync
       Ollama → force-refresh from /api/tags
               + persist to ollama_models table (updates global pool)
       Gemini → 400 "Use POST /v1/gemini/models/sync"
       → { models, synced: true }

GET    /v1/providers/{id}/key         → { api_key: "AIza…" } (decrypted, admin only)

GET    /v1/providers/{id}/selected-models
       Ollama → per-provider model list (ollama_models) merged with provider_selected_models
               default is_enabled = true for rows not yet in selection table
       Gemini → global gemini_models merged with provider_selected_models (default false)
       → { models: [{ model_name, is_enabled, synced_at }, ...] }

PATCH  /v1/providers/{id}/selected-models/{model_name}
       { is_enabled: bool } → 204
       (shared with Gemini — same handler, same table)
```

### Global Model Pool (ollama_model_handlers.rs)

```
GET  /v1/ollama/models         → { models: ["llama3", "mistral", ...] }  // distinct, sorted
POST /v1/ollama/models/sync    → 202 { job_id, status: "running" }       // async, no retry
GET  /v1/ollama/sync/status    → OllamaSyncJob (progress + per-provider results)
```

→ See `docs/llm/backend/backends-ollama-models.md` for full spec.

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
    pub api_key: Option<String>,          // "" or null → keeps existing key
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<Option<i16>>,   // Some(None) → clears FK
    pub server_id: Option<Option<Uuid>>,  // Some(None) → clears FK
    pub is_active: Option<bool>,
    pub is_free_tier: Option<bool>,
}
```

SQL for PATCH: `COALESCE($3, api_key_encrypted)` preserves existing key when `api_key = ""`.

---

## Provider Registry Caching

`queue_dispatcher_loop` calls `list_all()` on every job dequeue. Under high throughput this becomes hundreds of Postgres queries/second.

`CachingProviderRegistry` wraps `PostgresProviderRegistry` with a 5-second TTL in-memory cache:

```rust
// persistence/caching_provider_registry.rs
pub struct CachingProviderRegistry {
    inner: Arc<dyn LlmProviderRegistry>,
    cache: tokio::sync::RwLock<Option<(Vec<LlmProvider>, Instant)>>,
    ttl:   Duration,   // default: 5s
}
```

- **`list_all()`**: shared read lock fast path → return cached if fresh; write lock + DB query on miss.
  Double-checked locking prevents thundering herd on simultaneous cache misses.
- **Mutating methods** (`register`, `update_status`, `update`, `deactivate`): forward to inner + invalidate cache.
- **Read-only methods** (`list_active`, `get`): forward directly (called infrequently, no cache needed).

Wired in `main.rs`:
```rust
let provider_registry: Arc<dyn LlmProviderRegistry> = Arc::new(
    CachingProviderRegistry::new(
        Arc::new(PostgresProviderRegistry::new(pg_pool.clone())),
        Duration::from_secs(5),
    )
);
```

---

## VRAM-Aware Routing + Dynamic Concurrency

```
queue_dispatcher_loop (BLPOP [veronex:queue:jobs, veronex:queue:jobs:test]):
  1. list_active() → all active providers; VRAM check → sort by available VRAM
  2. For each candidate provider:
     a. thermal.get(provider_id) → skip if Hard; skip if Soft+active_slots>0
     b. slot_map.try_acquire(provider_id, model_name) → OwnedSemaphorePermit (non-blocking)
     c. If acquired → tokio::spawn run_job(permit)
        permit.drop() on task exit → slot auto-released (RAII)
  3. No slot acquired → LPUSH back to queue, sleep 2s

VRAM rules:
  total_vram_mb == 0 → always dispatchable (unlimited)
  total_vram_mb > 0  → prefer provider with most available VRAM

Concurrency slots (ConcurrencySlotMap):
  Default: 1 slot per (provider, model) until capacity analyzer runs
  Updated every ~5 min by capacity analyzer using Ollama /api/ps + /api/show + throughput stats
  Range: 1–8 slots; updated atomically (Semaphore replacement; in-flight permits unaffected)

→ Full concurrency + thermal spec: `docs/llm/backend/capacity.md`

Model selection filter (pick_best_provider):
  After VRAM candidate list: list_enabled(provider_id) from model_selection_repo
  Non-empty list + model not in list → skip provider (disabled)
  Empty list or error → include provider (backward compat — no restriction)
→ Full model selection spec: `docs/llm/backend/backends-ollama-models.md`
```

---

## Background Health Checker (health_checker.rs)

- Interval: 30 seconds, `start_health_checker()` called in `main.rs`
- Ollama: `GET /api/tags` → Online | Offline
- Gemini: `POST /v1beta/models/gemini-2.0-flash:generateContent` (minimal prompt)
- Status change → `UPDATE llm_providers SET status = ?`
- After hw_metrics load: `thermal.update(provider_id, temp_c)` → Normal/Soft/Hard
  - Sets/removes `veronex:throttle:{provider_id}` in Valkey (TTL 90s) for external observability

---

---

## OllamaAdapter — Streaming Protocol (`ollama/adapter.rs`)

`stream_tokens()` dispatches based on `job.messages`:

```rust
fn stream_tokens(&self, job: &InferenceJob) -> Pin<Box<dyn Stream<...>>> {
    if let Some(messages) = &job.messages {
        return self.stream_chat(job.model_name.as_str(), messages.clone());
    }
    self.stream_generate(job.model_name.as_str(), job.prompt.as_str())
}
```

| Condition | Endpoint | Used by |
|-----------|----------|---------|
| `job.messages = None` | `POST /api/generate` | `POST /v1/inference` (VeronexNative) |
| `job.messages = Some(...)` | `POST /api/chat` | All compat handlers (OpenAI, Ollama, Gemini) |

### Context Length (`num_ctx`) per Request

Every Ollama request includes `options.num_ctx` derived from `model_effective_num_ctx(model_name)`:

```rust
fn model_effective_num_ctx(model: &str) -> u32 {
    let m = model.to_lowercase();
    if m.contains("200k")                     { return 204_800; }
    if m.contains("128k")                     { return 131_072; }
    if m.contains("1m")                       { return 131_072; } // 1M models: 128K practical limit
    if m.contains("72b") || m.contains("70b") { return  32_768; }
    32_768 // default for 7B–32B models
}
```

This per-request override ensures each model uses its natural context window regardless of the global `OLLAMA_CONTEXT_LENGTH` env var on the Ollama server.

**Why this matters**: Without `options.num_ctx`, all models fall back to `OLLAMA_CONTEXT_LENGTH` (e.g. `8192`). A 128K model receiving a 24K-token conversation gets silently truncated → model gives incomplete answers → client retries with growing context → retry storm.

**Dual protection** (belt + suspenders):
1. GitOps: `OLLAMA_CONTEXT_LENGTH: 204800` on Ollama StatefulSet (global floor)
2. Veronex: `options.num_ctx` per request (model-specific override)

### `/api/generate` — single prompt

```json
{ "model": "qwen3:8b", "prompt": "...", "stream": true, "think": false, "options": {"num_ctx": 32768} }
```

```rust
struct GenerateResponse {
    response: String,
    done: bool,
    done_reason: Option<String>,   // "stop" | "load" | "length"
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}
```

### `/api/chat` — multi-turn messages

```json
{
  "model": "qwen3:8b",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user",   "content": "..."},
    {"role": "assistant", "content": "..."},
    {"role": "user",   "content": "..."}
  ],
  "stream": true,
  "think": false,
  "options": {"num_ctx": 32768}
}
```

```rust
struct ChatChunk {
    message: Option<ChatChunkMessage>,  // { content: Option<String> }
    done: bool,
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}
```

### `think: false`

Disables extended thinking (qwen3 and similar). Without it, thinking tokens
(`<think>…</think>`) inflate `eval_count` and appear in the token stream.
Non-thinking models silently ignore the field.

### `done_reason: "load"` handling

When Ollama first loads a model into VRAM it emits an intermediate chunk with `done_reason: "load"`.
Both `stream_generate()` and `stream_chat()` skip these chunks and keep reading.
Without this fix, the stream terminates prematurely with empty output.

### Token Count Accuracy

| Scenario | `eval_count` value |
|----------|--------------------|
| `think: false` (all models) | visible output tokens only ✓ |
| `think: true` (qwen3 default) | thinking + output tokens (inflated ✗) |

### Format conversion (compat handlers → Ollama messages)

| Entry route | Converter | Notes |
|-------------|-----------|-------|
| `POST /v1/chat/completions` | `ChatMessage::into_ollama_value()` | OpenAI `tool_calls[].arguments` (JSON string) → Ollama (JSON object) |
| `POST /api/chat` | passthrough (already Ollama format) | — |
| `POST /v1beta/models/*` | `contents_to_ollama()` | Gemini `role: "model"` → `"assistant"`, `functionCall`/`functionResponse` mapped |
| `POST /v1/test/*` | passthrough or extract prompt | Test Run handlers pass simple messages or None |

---

## Web UI

→ See `docs/llm/frontend/web-providers.md` → OllamaTab + OllamaSyncSection
