# Architecture

> Hexagonal Architecture overview | **Last Updated**: 2026-03-03

## Structure

```
crates/veronex/src/
├── domain/          # Entities, value objects, enums (no deps)
├── application/     # Use cases + ports (traits)
│   ├── ports/
│   │   ├── inbound/   # InferenceUseCase
│   │   └── outbound/  # Repositories, registries, adapters
│   └── use_cases/
├── infrastructure/  # Adapters (implements ports)
│   ├── inbound/http/  # Axum handlers, middleware, router
│   └── outbound/      # Postgres, Valkey, Ollama, Gemini, OTel
└── main.rs          # Composition root (wires everything)
```

## Dependency Rule

```
infrastructure → application → domain
(Never reverse. Domain knows nothing outside itself.)
```

## Key Ports

| Port                           | Direction | Implemented By                      |
| ------------------------------ | --------- | ------------------------------------ |
| `InferenceUseCase`             | Inbound   | HTTP handlers (inference + OpenAI)   |
| `InferenceBackendPort`         | Outbound  | OllamaAdapter / GeminiAdapter        |
| `LlmProviderRegistry`          | Outbound  | PostgresProviderRegistry + CachingProviderRegistry (5s TTL decorator) |
| `GpuServerRegistry`            | Outbound  | PostgresGpuServerRegistry            |
| `JobRepository`                | Outbound  | PostgresJobRepository                |
| `ApiKeyRepository`             | Outbound  | PostgresApiKeyRepository             |
| `AccountRepository`            | Outbound  | PostgresAccountRepository            |
| `SessionRepository`            | Outbound  | PostgresSessionRepository            |
| `AuditPort`                    | Outbound  | HttpAuditAdapter → veronex-analytics (fail-open) |
| `ObservabilityPort`            | Outbound  | HttpObservabilityAdapter → veronex-analytics (fail-open) |
| `AnalyticsRepository`          | Outbound  | HttpAnalyticsClient → veronex-analytics |
| `ModelManagerPort`             | Outbound  | OllamaModelManager (LRU eviction)    |
| `GeminiPolicyRepository`       | Outbound  | PostgresGeminiPolicyRepository       |
| `GeminiSyncConfigRepository`   | Outbound  | PostgresGeminiSyncConfigRepository   |
| `GeminiModelRepository`        | Outbound  | PostgresGeminiModelRepository        |
| `ProviderModelSelectionRepository` | Outbound | PostgresProviderModelSelectionRepository |
| `OllamaModelRepository`           | Outbound  | PostgresOllamaModelRepository             |
| `OllamaSyncJobRepository`         | Outbound  | PostgresOllamaSyncJobRepository           |
| `ModelCapacityRepository`         | Outbound  | PostgresModelCapacityRepository           |
| `CapacitySettingsRepository`      | Outbound  | PostgresCapacitySettingsRepository        |
| `LabSettingsRepository`           | Outbound  | PostgresLabSettingsRepository (lab feature flags) |
| `MessageStore`                    | Outbound  | S3MessageStore (MinIO/AWS S3) — `messages/{job_id}.json` |
| `QueuePort`                       | Outbound  | Valkey (BLPOP/RPUSH via fred 10)          |
| `StreamPort`                      | Outbound  | In-memory buffer + tokio Notify           |

## HTTP Auth Layers

```
Public         /v1/setup/*, /v1/auth/*                                         no middleware
API Key Auth   /v1/chat/*, /v1/inference/*, /api/*, /v1beta/*                  api_key_auth + rate_limiter
JWT Bearer     /v1/accounts/*, /v1/sessions/*, /v1/audit                       jwt_auth → RequireSuper extractor
JWT Bearer     /v1/test/*                                                       jwt_auth (no rate limit, account_id tracking)
JWT Bearer     /v1/keys/*, /v1/usage/*, /v1/dashboard/*, /v1/backends/*,
               /v1/servers/*, /v1/gemini/*, /v1/ollama/*                       jwt_auth (admin operations)
```

Dashboard admin endpoints:
- `GET/PATCH /v1/dashboard/capacity/settings` — capacity analyzer config
- `POST /v1/dashboard/capacity/sync` — manual capacity analysis; 202 Accepted / 409 Conflict (already running)
- `POST /v1/dashboard/session-grouping/trigger` — manual session grouping; body `{ before_date?: "YYYY-MM-DD" }`; 202 OK / 409 Conflict (already running)
- `GET /v1/usage/{key_id}/models?hours=N` — per-key model breakdown

`/v1/setup/status` + `POST /v1/setup` — no auth, first-run only. `POST /v1/setup` returns 409 if any account exists.

- `jwt_auth`: extracts `Authorization: Bearer`, decodes HS256 → inserts `Claims { sub, role, jti, exp }` into extensions
- `RequireSuper`: `FromRequestParts` — reads Claims, returns 403 if `role != "super"`

## Inference Flow

All inference entry points route through the Valkey queue — no handler sends directly to Ollama.

```
── API Key routes (X-API-Key / Authorization: Bearer / x-goog-api-key) ──────────────
POST /v1/chat/completions           → openai_handlers    (ApiFormat::OpenaiCompat)
POST /v1/inference                  → handlers           (ApiFormat::VeronexNative)
POST /api/generate                  → ollama_compat_handlers (ApiFormat::OllamaNative)
POST /api/chat                      → ollama_compat_handlers (ApiFormat::OllamaNative)
POST /v1beta/models/{*}:streamGenerateContent → gemini_compat_handlers (ApiFormat::GeminiNative)
POST /v1beta/models/{*}:generateContent       → gemini_compat_handlers (ApiFormat::GeminiNative)

── Test Run routes (Bearer JWT, no API key) ──────────────────────────────────────────
POST /v1/test/completions           → test_handlers  (ApiFormat::OpenaiCompat)
POST /v1/test/api/chat              → test_handlers  (ApiFormat::OllamaNative)
POST /v1/test/api/generate          → test_handlers  (ApiFormat::OllamaNative)
POST /v1/test/v1beta/models/{*}     → test_handlers  (ApiFormat::GeminiNative)

── Common path (all routes) ──────────────────────────────────────────────────────────
InferenceUseCaseImpl::submit(prompt, model, provider_type, api_key_id?, account_id?,
                              source, api_format, messages?, tools?,
                              request_path?, conversation_id?)
  → Valkey RPUSH veronex:queue:jobs:paid   (source=Api, tier=paid)
               or veronex:queue:jobs        (source=Api, tier=free/standard)
               or veronex:queue:jobs:test  (source=Test)
  → SSE/NDJSON stream → Client (format determined by handler, not dispatcher)

queue_dispatcher_loop (BLPOP [paid queue, API queue, test queue]):
  Paid queue polled first, then standard API, then test (BLPOP key-order guarantee)
  → VRAM check + thermal check + slot_map.try_acquire()
  → OllamaAdapter.stream_tokens():
      job.messages.is_some() → POST /api/chat  (multi-turn)
      job.messages.is_none() → POST /api/generate  (single prompt)
      always sends options.num_ctx = model_effective_num_ctx(model_name)
        "128k" → 131072 | "200k" → 204800 | "1m" → 131072 | "70b/72b" → 32768 | default → 32768
  → HttpObservabilityAdapter → veronex-analytics → OTel → Redpanda → ClickHouse

SSE reconnect:
  [API Key Test] GET /v1/jobs/{id}/stream          (X-API-Key)
  [Test Run]     GET /v1/test/jobs/{id}/stream     (Bearer JWT)
  → OpenAI SSE replay (completed or live)
  Frontend: localStorage veronex:test:tab:{mode}:{tabKey} → auto-reconnect on mount

Job source/format tracking:
  API Key Test: api_key_id = key.id, account_id = NULL, source = Api
  Test Run:     api_key_id = NULL,   account_id = claims.sub, source = Test
  api_format:   set per route (OpenaiCompat | OllamaNative | GeminiNative | VeronexNative)
  conversation_id: X-Conversation-ID header (realtime) OR batch-assigned by session_grouping_loop (daily)
  messages_hash:        Blake2b-256 of full messages array — set at save time (migration 000048)
  messages_prefix_hash: Blake2b-256 of messages[0..-1] — "" for first turn; used by grouping loop
  messages_json:   FULL input context — S3 PRIMARY (messages/{job_id}.json); DB NULL for new jobs; DB fallback for legacy jobs
  tool_calls_json: model-returned tool calls — persisted JSONB (migration 000043)
  messages:        Some(json) → routes to /api/chat; uploaded to S3 in submit(); cleared from in-memory job after stream_tokens()
```

## AppState (main.rs — Composition Root)

All state injected via `Arc<dyn Trait>` into Axum `State<AppState>`:
- `use_case`, `job_repo`, `api_key_repo`, `provider_registry`, `gpu_server_registry`
- `ollama_model_repo`, `ollama_sync_job_repo`
- `gemini_policy_repo`, `gemini_sync_config_repo`, `gemini_model_repo`
- `model_selection_repo`, `pg_pool`
- `cpu_snapshot_cache: Arc<DashMap<Uuid, CpuSnapshot>>` — GPU snapshot per server (DashMap; no lock)
- `valkey_pool` (rate limiting + queue + JWT revocation blocklist)
- `account_repo`, `session_repo` (RBAC + sessions)
- `audit_port` → `HttpAuditAdapter` (fail-open, POST to veronex-analytics)
- `observability` → `HttpObservabilityAdapter` (fail-open, POST to veronex-analytics)
- `analytics_repo` → `HttpAnalyticsClient` (GET from veronex-analytics)
- `jwt_secret: String` (HS256 signing key)
- `slot_map: Arc<ConcurrencySlotMap>` — per-(backend, model) semaphores (VRAM-aware)
- `thermal: Arc<ThermalThrottleMap>` — 30s thermal throttle state (Normal/Soft/Hard)
- `capacity_repo: Arc<dyn ModelCapacityRepository>` — VRAM + throughput DB
- `capacity_settings_repo: Arc<dyn CapacitySettingsRepository>` — analysis config
- `capacity_manual_trigger: Arc<Notify>` — instant analysis trigger
- `capacity_analysis_lock: Arc<Semaphore(1)>` — prevents concurrent capacity analysis runs (409 if already active)
- `analyzer_url: String` — Ollama URL for capacity LLM (CAPACITY_ANALYZER_OLLAMA_URL)
- `lab_settings_repo: Arc<dyn LabSettingsRepository>` — experimental feature flags (SSOT: `docs/llm/backend/lab_features.md`)
- `message_store: Option<Arc<dyn MessageStore>>` — S3MessageStore (None when S3_ENDPOINT unset)
- `session_grouping_lock: Arc<Semaphore(1)>` — prevents concurrent session grouping runs (409 if already active)

## Background Loops

| Loop | Interval | Purpose |
|------|----------|---------|
| `health_checker` | 30 s | Backend online/offline + thermal update |
| `run_capacity_analysis_loop` | 30 s tick (runs per DB interval) | KV cache calc + LLM slot recommendation; holds `capacity_analysis_lock` during each run |
| `queue_dispatcher_loop` | BLPOP 5s | VRAM-sorted job dispatch + slot acquire |
| `run_session_grouping_loop` | 24 h (`SESSION_GROUPING_INTERVAL_SECS`) | Batch-assign `conversation_id` via `messages_prefix_hash` chaining — no LLM; date cutoff = today (`created_at < DATE_TRUNC('day', NOW())`); skips tick if `session_grouping_lock` held |

## Dynamic Concurrency

`ConcurrencySlotMap` → `(provider_id, model_name) → Semaphore(N)` where N = `OLLAMA_NUM_PARALLEL` (default 1)
- Capacity analyzer updates N every 5 min via `/api/ps` VRAM + `/api/show` KV cache formula
- Thermal throttle gates dispatch: Normal < 78°C / Soft ≥ 85°C / Hard ≥ 92°C
- RAII permit drop auto-releases slot for next job
- **SSOT**: `docs/llm/backend/capacity.md`

**SSOT**: `docs/llm/policies/architecture.md`
