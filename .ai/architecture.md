# Architecture

> Hexagonal Architecture overview | **Last Updated**: 2026-03-02 (rev: submit() sig tools/request_path/conversation_id; queue depth endpoint added)

## Structure

```
crates/inferq/src/
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
| `LlmBackendRegistry`           | Outbound  | PostgresBackendRegistry + CachingBackendRegistry (5s TTL decorator) |
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
| `BackendModelSelectionRepository` | Outbound | PostgresBackendModelSelectionRepository |
| `OllamaModelRepository`           | Outbound  | PostgresOllamaModelRepository             |
| `OllamaSyncJobRepository`         | Outbound  | PostgresOllamaSyncJobRepository           |
| `ModelCapacityRepository`         | Outbound  | PostgresModelCapacityRepository           |
| `CapacitySettingsRepository`      | Outbound  | PostgresCapacitySettingsRepository        |
| `LabSettingsRepository`           | Outbound  | PostgresLabSettingsRepository (lab feature flags) |
| `QueuePort`                       | Outbound  | Valkey (BLPOP/RPUSH via fred 10)          |
| `StreamPort`                      | Outbound  | In-memory buffer + tokio Notify           |

## HTTP Auth Layers

```
Public         /v1/setup/*, /v1/auth/*     no middleware
API Key Auth   /v1/chat/*, /v1/inference/* api_key_auth + rate_limiter
JWT Bearer     /v1/accounts/*, /v1/audit   jwt_auth middleware → RequireSuper extractor
JWT Bearer     /v1/test/*                  jwt_auth (no rate limit, account_id tracking)
```

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
InferenceUseCaseImpl::submit(prompt, model, backend_type, api_key_id?, account_id?,
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
  conversation_id: from X-Conversation-ID header — groups all turns of one agent session
  messages_json:   FULL input context (system + file contents + history) — persisted JSONB (migration 000045)
  tool_calls_json: model-returned tool calls — persisted JSONB (migration 000043)
  messages:        Some(json) → routes to /api/chat; persisted as messages_json in DB
```

## AppState (main.rs — Composition Root)

All state injected via `Arc<dyn Trait>` into Axum `State<AppState>`:
- `use_case`, `job_repo`, `api_key_repo`, `backend_registry`, `gpu_server_registry`
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
- `analyzer_url: String` — Ollama URL for capacity LLM (CAPACITY_ANALYZER_OLLAMA_URL)
- `lab_settings_repo: Arc<dyn LabSettingsRepository>` — experimental feature flags (SSOT: `docs/llm/backend/lab_features.md`)

## Background Loops

| Loop | Interval | Purpose |
|------|----------|---------|
| `health_checker` | 30 s | Backend online/offline + thermal update |
| `run_capacity_analysis_loop` | 30 s tick (runs per DB interval) | KV cache calc + LLM slot recommendation |
| `queue_dispatcher_loop` | BLPOP 5s | VRAM-sorted job dispatch + slot acquire |

## Dynamic Concurrency (feat: api-key-usage)

Old: `busy_backends: HashSet<Uuid>` → max 1 job/backend, no VRAM awareness

New: `ConcurrencySlotMap` → `(backend_id, model_name) → Semaphore(N)` where N ∈ [1,8]
- Capacity analyzer updates N every 5 min using `/api/ps` VRAM + `/api/show` KV formula
- Thermal throttle gates dispatch: Soft=cap new, Hard=block all
- RAII permit drop auto-releases slot for next job
- SSOT: `docs/llm/backend/capacity.md`

**SSOT**: `docs/llm/policies/architecture.md`
