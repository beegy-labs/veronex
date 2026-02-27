# Backends — Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to backend API request/response | `backend_handlers.rs` → `RegisterBackendRequest` / `UpdateBackendRequest` + migration |
| Change VRAM dispatch algorithm | `infrastructure/outbound/backend_router.rs` → `dispatch()` function |
| Change health check logic | `infrastructure/outbound/health_checker.rs` → `check_backend()` |
| Add new model management endpoint | `backend_handlers.rs` + `router.rs` |
| Change how busy_backends works | `backend_router.rs` → `busy_backends: Arc<Mutex<HashSet<Uuid>>>` |
| Add new LlmBackend DB column | `migrations/` new file + `domain/entities/llm_backend.rs` + `persistence/backend_registry.rs` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/llm_backend.rs` | `LlmBackend` entity |
| `crates/inferq/src/application/ports/outbound/` | `LlmBackendRegistry` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/backend_registry.rs` | `PostgresBackendRegistry` |
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

## VRAM-Aware Routing (backend_router.rs)

```
queue_dispatcher_loop (BLPOP veronex:queue:jobs):
  1. list_active() → all active backends
  2. Ollama: GET /api/ps → available VRAM = total - used
  3. busy_backends: Arc<Mutex<HashSet<Uuid>>> → exclude in-flight
  4. Best candidate claimed → busy_backends.insert(id)
  5. tokio::spawn run_job() → on finish: busy_backends.remove(id)
  6. No backend free → LPUSH back, sleep 2s

VRAM rules:
  total_vram_mb == 0 → always dispatchable (unlimited)
  total_vram_mb > 0  → prefer backend with most available VRAM
```

---

## Background Health Checker (health_checker.rs)

- Interval: 30 seconds, `start_health_checker()` called in `main.rs`
- Ollama: `GET /api/tags` → Online | Offline
- Gemini: `POST /v1beta/models/gemini-2.0-flash:generateContent` (minimal prompt)
- Status change → `UPDATE llm_backends SET status = ?`

---

## Web UI

→ See `docs/llm/frontend/web-backends.md` → OllamaTab + OllamaSyncSection
