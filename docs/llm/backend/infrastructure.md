# Infrastructure — Services, Ports & Env Vars

> SSOT | **Last Updated**: 2026-03-02 (rev: capacity control, thermal throttle, migrations 36-40)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new service | `docker-compose.yml` + `helm/veronex/templates/` | New service block + Helm Deployment/Service |
| Add new env var | `crates/inferq/src/main.rs` + `docker-compose.yml` + `web/.env.local.example` | Read in `main()`, set in compose `environment:`, document here |
| Add new Valkey key pattern | `infrastructure.rs` (this file) + relevant handler | Add to Valkey Key Patterns table, use `veronex:` prefix |
| Add new DB migration | `crates/inferq/migrations/` new `.sql` file | Name: `{next_number}_description.sql`; add row to migration list in this file |
| Add new repo to AppState | `infrastructure/inbound/http/state.rs` + `crates/inferq/src/main.rs` | Add `Arc<dyn Trait>` field to `AppState`, init in `main()` composition root |
| Change host port mapping | `docker-compose.yml` `ports:` + update memory/docs | Remember offset convention: +1 from standard (5432→5433, 6379→6380) |

## Key Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | Local dev all-in-one |
| `crates/inferq/src/main.rs` | Composition root (all adapters wired) |
| `crates/inferq/src/infrastructure/inbound/http/state.rs` | `AppState` struct |
| `crates/inferq/migrations/` | All DB migrations |
| `helm/veronex/` | Kubernetes Helm chart |

---

## Data Pipeline

```
veronex ──→ veronex-analytics ──→ OTel Logs → OTel Collector → Redpanda [otel-logs]    → otel_logs (MV)
OTel Collector (prometheus)   ──→ kafka/metrics               → Redpanda [otel-metrics] → otel_metrics_gauge (MV)
OTel Collector (otlp traces)  ──→ kafka/traces                → Redpanda [otel-traces]  → otel_traces_raw (MV)
veronex ──→ veronex-analytics ──→ GET /internal/* ──→ ClickHouse otel_logs / otel_metrics_gauge
```

- **Redpanda** = single message bus — Kafka 100% compatible; swap `kafka_broker_list` to migrate
- **ClickHouse** = read layer only — Kafka Engine pulls from Redpanda, MV writes into MergeTree
- **veronex-analytics** = internal write+read service; veronex has no direct Redpanda/ClickHouse dependency
- `docker/clickhouse/init.sql` — all tables: MergeTree targets first, then Kafka Engine + Materialized Views

→ Full pipeline spec: `docs/llm/backend/infrastructure-otel.md`

## Services

| Service | Image | Host Port | Role |
|---------|-------|-----------|------|
| postgres | postgres:18-alpine | **5433** | Main DB — PG18, native `uuidv7()` |
| valkey | valkey/valkey:8-alpine | **6380** | Queue (BLPOP), rate limiting, JWT revocation blocklist |
| clickhouse | clickhouse-server:26.1 | 8123, 9000 | Analytics read layer — `otel_logs`, `otel_metrics_gauge` |
| redpanda | redpandadata/redpanda:v24.2.7 | 9092 | Single message bus (Kafka-compatible) |
| veronex | local build | **3001**→3000 | Rust API server (crate: `veronex`) |
| veronex-analytics | local build | internal 3003 | Analytics service — OTel write + ClickHouse read |
| veronex-web | local build | 3002 | Next.js admin dashboard |
| otel-collector | docker/otel/Dockerfile | 4317, 4318, 13133 | Metrics + traces + logs collection → Redpanda |

> Port offsets (+1): 5432→5433, 6379→6380, 3000→3001 (vergate/Gitea conflicts)

---

## Environment Variables

```bash
# Rust API (veronex)
DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
VALKEY_URL=redis://localhost:6380
OLLAMA_URL=http://localhost:11434        # legacy default; backends now stored in DB
GEMINI_API_KEY=<optional legacy>         # per-backend keys stored in DB
BOOTSTRAP_API_KEY=veronex-bootstrap-admin-key
PORT=3000                                # container internal port
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317  # optional (traces only)

# Auth (JWT)
JWT_SECRET=change-me-in-production       # HS256 key — MUST change in production
# BOOTSTRAP_SUPER_USER=<username>        # optional: pre-seed super account (CI/automated)
# BOOTSTRAP_SUPER_PASS=<password>        # optional: omit to use first-run setup flow

# Capacity analyzer (dynamic concurrency)
CAPACITY_ANALYZER_OLLAMA_URL=http://localhost:11434  # default: same as OLLAMA_URL
# analyzer_model configured via DB: PATCH /v1/dashboard/capacity/settings (default: qwen2.5:3b)

# Analytics service
ANALYTICS_URL=http://localhost:3003      # docker: http://veronex-analytics:3003
ANALYTICS_SECRET=<shared-secret>         # Bearer token for internal API auth

# veronex-analytics (internal service)
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_USER=veronex
CLICKHOUSE_PASSWORD=veronex
CLICKHOUSE_DB=veronex
OTEL_HTTP_ENDPOINT=http://otel-collector:4318   # OTLP HTTP (not gRPC)
ANALYTICS_SECRET=<shared-secret>

# Next.js web (veronex-web)
NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
NEXT_PUBLIC_VERONEX_ADMIN_KEY=veronex-bootstrap-admin-key
```

---

## Valkey Key Patterns

```
veronex:queue:jobs                              # inference job queue — API key requests (RPUSH/BLPOP)
veronex:queue:jobs:test                         # inference job queue — test run requests (lower priority)
veronex:ratelimit:rpm:{key_id}:{minute}         # API key RPM sorted set
veronex:ratelimit:tpm:{key_id}:{minute}         # API key TPM counter
veronex:gemini:rpm:{backend_id}:{model}:{min}   # Gemini per-backend RPM
veronex:gemini:rpd:{backend_id}:{model}:{date}  # Gemini per-backend RPD
veronex:gemini:models:{backend_id}              # Gemini model list cache (TTL 1h)
veronex:revoked:{jti}                           # JWT revocation blocklist (TTL = remaining token lifetime)
veronex:pwreset:{raw_token}                     # password-reset one-time token (TTL 24h)
veronex:throttle:{backend_id}                   # thermal Hard throttle flag (TTL 90s, set by health_checker)
veronex:hw:{backend_id}                         # hw_metrics JSON (temp_c, vram_used_mb, etc., TTL ~60s)
```

> Concurrency slots are **in-process** `Arc<Semaphore>` in `ConcurrencySlotMap` (NOT Valkey).

---

## DB Migrations (crates/inferq/migrations/)

| Migration | Description |
|-----------|-------------|
| 000001 | api_keys CREATE |
| 000002 | inference_jobs CREATE |
| 000003 | llm_backends CREATE |
| 000004 | jobs: add result_text |
| 000005 | backends: add agent_url |
| 000006 | backends: add gpu_index |
| 000007 | backends: add total_ram_mb (legacy) |
| 000008 | backends: add node_exporter_url (moved to gpu_servers) |
| 000009 | gpu_servers CREATE |
| 000010 | backends: add server_id FK |
| 000011 | backends: drop node_exporter_url + total_ram_mb |
| 000012 | gpu_servers: drop host |
| 000013 | gpu_servers: drop total_ram_mb |
| 000014 | jobs: add api_key_id FK |
| 000015 | jobs: add latency_ms |
| 000016 | backends: add is_free_tier, rpm_limit, rpd_limit (rpm/rpd removed in 018) |
| 000017 | gemini_rate_limit_policies CREATE + seed |
| 000018 | backends: drop rpm_limit, rpd_limit |
| 000019 | policies: add available_on_free_tier |
| 000020 | jobs: add ttft_ms, completion_tokens |
| 000021 | api_keys: add deleted_at (soft-delete) |
| 000022 | backend_selected_models CREATE |
| 000023 | gemini_sync_config CREATE |
| 000024 | gemini_models CREATE |
| 000025 | gemini_rate_limit_policies: update free-tier limits to 2026-02 values |
| 000026 | ollama_models CREATE (PK: model_name + backend_id, FK → llm_backends) |
| 000027 | ollama_sync_jobs CREATE (async background sync tracking) |
| 000028 | SET DEFAULT uuidv7() on all UUID PKs (PG18 native; replaces gen_random_uuid()) |
| 000029 | jobs: add prompt_tokens |
| 000030 | jobs: add cached_tokens |
| 000031 | jobs: add source (api \| test) |
| 000032 | api_keys: unique (tenant_id, name) constraint |
| 000033 | api_keys: add key_type (standard \| test) |
| 000034 | accounts CREATE (RBAC: super \| admin, soft-delete, Argon2id password_hash) |
| 000035 | api_keys: add account_id FK + is_test_key + unique partial index |
| 000036 | account_sessions CREATE (jti, refresh_token_hash BLAKE2b, rolling sessions) |
| 000037 | inference_jobs: add account_id FK (test run tracking) |
| 000038 | api_keys: add tier column |
| 000039 | model_capacity CREATE (PK: backend_id+model_name); inference_jobs: add backend_id FK; capacity_settings singleton (id=1) |
| 000040 | api_keys: drop `uq_api_keys_tenant_name` — name is a non-unique label; UUIDv7 `id` is the unique identifier |

---

## UUID Policy

All primary keys use **UUIDv7** — time-ordered, k-sortable, monotonically increasing.

| Layer | How |
|-------|-----|
| Application (Rust) | `Uuid::now_v7()` generated before every INSERT |
| Database (PG18) | `DEFAULT uuidv7()` on all UUID PK columns (fallback, migration 028) |
| ClickHouse | UUID columns receive UUIDv7 from app — no DB-level generation |

> **Never use `Uuid::new_v4()` or `gen_random_uuid()`** for primary keys.
> The one exception: Valkey sorted-set members use `Uuid::now_v7()` (not PKs).

---

## AppState (state.rs)

```rust
pub struct AppState {
    // Inference core
    pub use_case:                  Arc<dyn InferenceUseCase>,
    pub job_repo:                  Arc<dyn JobRepository>,
    pub api_key_repo:              Arc<dyn ApiKeyRepository>,
    // Backend routing
    pub backend_registry:          Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry:       Arc<dyn GpuServerRegistry>,
    pub ollama_model_repo:         Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo:      Arc<dyn OllamaSyncJobRepository>,
    pub gemini_policy_repo:        Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo:   Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo:         Arc<dyn GeminiModelRepository>,
    pub model_selection_repo:      Arc<dyn BackendModelSelectionRepository>,
    // Auth / RBAC
    pub account_repo:              Arc<dyn AccountRepository>,
    pub session_repo:              Arc<dyn SessionRepository>,
    pub jwt_secret:                String,
    // Observability + analytics
    pub audit_port:                Arc<dyn AuditPort>,           // fail-open HttpAuditAdapter
    pub observability:             Arc<dyn ObservabilityPort>,   // fail-open HttpObservabilityAdapter
    pub analytics_repo:            Arc<dyn AnalyticsRepository>, // HttpAnalyticsClient
    // Dynamic concurrency + thermal
    pub slot_map:                  Arc<ConcurrencySlotMap>,
    pub thermal:                   Arc<ThermalThrottleMap>,
    pub capacity_repo:             Arc<dyn ModelCapacityRepository>,
    pub capacity_settings_repo:    Arc<dyn CapacitySettingsRepository>,
    pub capacity_manual_trigger:   Arc<tokio::sync::Notify>,
    pub analyzer_url:              String,
    // Infrastructure
    pub valkey_pool:               Option<fred::clients::RedisPool>,
    pub pg_pool:                   sqlx::PgPool,
}
```

---

## Helm Chart (helm/veronex/)

```
helm/veronex/
├── Chart.yaml
├── values.yaml
└── templates/
    ├── veronex/           Deployment + Service (veronex binary)
    ├── veronex-analytics/ Deployment + Service (analytics internal service)
    ├── veronex-web/       Deployment + Service (Next.js)
    ├── postgres/          Deployment + Service + PVC
    ├── valkey/            Deployment + Service + PVC
    ├── clickhouse/        Deployment + Service + PVC
    ├── redpanda/          Deployment + Service
    └── otel-collector/    Deployment + Service + ConfigMap
```

**Setup flow**: No `BOOTSTRAP_SUPER_USER`/`BOOTSTRAP_SUPER_PASS` defaults.
Use `POST /v1/setup` (no auth) on first access to create the super admin account.

→ See `docs/llm/backend/infrastructure-otel.md` for OTel pipeline details.
