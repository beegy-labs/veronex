# Backends — Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-02 (rev: CachingBackendRegistry — 5s TTL cache wrapping PostgresBackendRegistry)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to backend API request/response | `backend_handlers.rs` → `RegisterBackendRequest` / `UpdateBackendRequest` + migration |
| Change VRAM dispatch algorithm | `infrastructure/outbound/backend_router.rs` → `dispatch()` function |
| Change health check logic | `infrastructure/outbound/health_checker.rs` → `check_backend()` |
| Add new model management endpoint | `backend_handlers.rs` + `router.rs` |
| Change concurrency slot allocation | `infrastructure/outbound/capacity/slot_map.rs` → `ConcurrencySlotMap::update_capacity()` |
| Change thermal throttle thresholds | `infrastructure/outbound/capacity/thermal.rs` → `ThermalThrottleMap::update()` |
| Add new LlmBackend DB column | `migrations/` new file + `domain/entities/llm_backend.rs` + `persistence/backend_registry.rs` |
| Change backend list cache TTL | `persistence/caching_backend_registry.rs` → `CachingBackendRegistry::new()` TTL arg in `main.rs` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/llm_backend.rs` | `LlmBackend` entity |
| `crates/inferq/src/application/ports/outbound/` | `LlmBackendRegistry` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/backend_registry.rs` | `PostgresBackendRegistry` (DB adapter) |
| `crates/inferq/src/infrastructure/outbound/persistence/caching_backend_registry.rs` | `CachingBackendRegistry` (5s TTL cache decorator) |
| `crates/inferq/src/infrastructure/outbound/ollama/adapter.rs` | `OllamaAdapter` (streaming) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `DynamicBackendRouter` + `queue_dispatcher_loop` |
| `crates/inferq/src/infrastructure/outbound/health_checker.rs` | 30s background health checker |
| `crates/inferq/src/infrastructure/inbound/http/backend_handlers.rs` | CRUD + model management handlers |

---

## LlmBackend Entity

```rust
// domain/entities/llm_backend.rs
pub struct LlmBackend {
    pub id: Uuid,
    pub name: String,
    pub backend_type: BackendType,         // Ollama | Gemini
    pub url: String,                       // "http://host:11434" (Ollama) | "" (Gemini)
    pub api_key_encrypted: Option<String>,
    pub is_active: bool,
    pub total_vram_mb: i64,               // 0 = unlimited
    pub gpu_index: Option<i16>,           // 0-based GPU index on host
    pub server_id: Option<Uuid>,          // FK → gpu_servers (Gemini = NULL)
    pub agent_url: Option<String>,        // Phase 2 sidecar (unused)
    pub is_free_tier: bool,               // Gemini only
    pub status: LlmBackendStatus,         // Online | Offline | Degraded
    pub registered_at: DateTime<Utc>,
}
```

## DB Schema

```sql
CREATE TABLE llm_backends (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    backend_type      VARCHAR(50)  NOT NULL,   -- 'ollama' | 'gemini'
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
-- migrations: 000003 CREATE, 000005 agent_url, 000006 gpu_index,
--             000007 total_ram_mb (legacy), 000010 server_id,
--             000011 drop node_exporter_url+total_ram_mb,
--             000016 is_free_tier, 000018 drop rpm/rpd limits
```

---

## API Endpoints (backend_handlers.rs)

```
POST   /v1/backends                   RegisterBackendRequest → RegisterBackendResponse
GET    /v1/backends                   → Vec<BackendSummary>
PATCH  /v1/backends/{id}             UpdateBackendRequest → 200
DELETE /v1/backends/{id}             → 204
POST   /v1/backends/{id}/healthcheck → { status: "online" | "offline" | "degraded" }

GET    /v1/backends/{id}/models
       Ollama → GET /api/tags (live)
       Gemini → 400 "Use GET /v1/gemini/models"
       → { models: Vec<String> }

POST   /v1/backends/{id}/models/sync
       Ollama → force-refresh from /api/tags
               + persist to ollama_models table (updates global pool)
       Gemini → 400 "Use POST /v1/gemini/models/sync"
       → { models, synced: true }

GET    /v1/backends/{id}/key         → { api_key: "AIza…" } (decrypted, admin only)
```

### Global Model Pool (ollama_model_handlers.rs)

```
GET  /v1/ollama/models         → { models: ["llama3", "mistral", ...] }  // distinct, sorted
POST /v1/ollama/models/sync    → 202 { job_id, status: "running" }       // async, no retry
GET  /v1/ollama/sync/status    → OllamaSyncJob (progress + per-backend results)
```

→ See `docs/llm/backend/backends-ollama-models.md` for full spec.

### Request Structs

```rust
pub struct RegisterBackendRequest {
    pub name: String,
    pub backend_type: BackendType,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<i16>,
    pub server_id: Option<Uuid>,
    pub is_free_tier: Option<bool>,
}

pub struct UpdateBackendRequest {
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

## Backend Registry Caching

`queue_dispatcher_loop` calls `list_all()` on every job dequeue. Under high throughput this becomes hundreds of Postgres queries/second.

`CachingBackendRegistry` wraps `PostgresBackendRegistry` with a 5-second TTL in-memory cache:

```rust
// persistence/caching_backend_registry.rs
pub struct CachingBackendRegistry {
    inner: Arc<dyn LlmBackendRegistry>,
    cache: tokio::sync::RwLock<Option<(Vec<LlmBackend>, Instant)>>,
    ttl:   Duration,   // default: 5s
}
```

- **`list_all()`**: shared read lock fast path → return cached if fresh; write lock + DB query on miss.
  Double-checked locking prevents thundering herd on simultaneous cache misses.
- **Mutating methods** (`register`, `update_status`, `update`, `deactivate`): forward to inner + invalidate cache.
- **Read-only methods** (`list_active`, `get`): forward directly (called infrequently, no cache needed).

Wired in `main.rs`:
```rust
let backend_registry: Arc<dyn LlmBackendRegistry> = Arc::new(
    CachingBackendRegistry::new(
        Arc::new(PostgresBackendRegistry::new(pg_pool.clone())),
        Duration::from_secs(5),
    )
);
```

---

## VRAM-Aware Routing + Dynamic Concurrency

```
queue_dispatcher_loop (BLPOP [veronex:queue:jobs, veronex:queue:jobs:test]):
  1. list_active() → all active backends; VRAM check → sort by available VRAM
  2. For each candidate backend:
     a. thermal.get(backend_id) → skip if Hard; skip if Soft+active_slots>0
     b. slot_map.try_acquire(backend_id, model_name) → OwnedSemaphorePermit (non-blocking)
     c. If acquired → tokio::spawn run_job(permit)
        permit.drop() on task exit → slot auto-released (RAII)
  3. No slot acquired → LPUSH back to queue, sleep 2s

VRAM rules:
  total_vram_mb == 0 → always dispatchable (unlimited)
  total_vram_mb > 0  → prefer backend with most available VRAM

Concurrency slots (ConcurrencySlotMap):
  Default: 1 slot per (backend, model) until capacity analyzer runs
  Updated every ~5 min by capacity analyzer using Ollama /api/ps + /api/show + throughput stats
  Range: 1–8 slots; updated atomically (Semaphore replacement; in-flight permits unaffected)

→ Full concurrency + thermal spec: `docs/llm/backend/capacity.md`
```

---

## Background Health Checker (health_checker.rs)

- Interval: 30 seconds, `start_health_checker()` called in `main.rs`
- Ollama: `GET /api/tags` → Online | Offline
- Gemini: `POST /v1beta/models/gemini-2.0-flash:generateContent` (minimal prompt)
- Status change → `UPDATE llm_backends SET status = ?`
- After hw_metrics load: `thermal.update(backend_id, temp_c)` → Normal/Soft/Hard
  - Sets/removes `veronex:throttle:{backend_id}` in Valkey (TTL 90s) for external observability

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

### `/api/generate` — single prompt

```json
{ "model": "qwen3:8b", "prompt": "...", "stream": true, "think": false }
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
  "think": false
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

→ See `docs/llm/frontend/web-backends.md` → OllamaTab + OllamaSyncSection
