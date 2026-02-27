# Infrastructure — Services, Ports & Env Vars

> SSOT | **Last Updated**: 2026-02-28 (rev: pg18 + ch26.2 + uuidv7)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new service | `docker-compose.yml` + `helm/inferq/templates/` | New service block + Helm Deployment/Service template |
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
| `helm/inferq/` | Kubernetes Helm chart |

---

## Services

| Service | Image | Host Port | Role |
|---------|-------|-----------|------|
| postgres | postgres:18-alpine | **5433** | Main DB — PG18, native `uuidv7()` |
| valkey | valkey/valkey:8-alpine | **6380** | Queue (BLPOP), rate limiting, model cache |
| clickhouse | clickhouse-server:26.1 | 8123, 9000 | inference_logs, OTel metrics/traces |
| redpanda | redpandadata/redpanda:v24.2.7 | 9092 | Kafka-compatible streaming buffer |
| veronex | local build | **3001**→3000 | Rust API server (crate: `veronex`) |
| veronex-web | local build | 3002 | Next.js admin dashboard |
| otel-collector | docker/otel/Dockerfile | 4317, 4318, 13133 | Metrics + trace collection |

> Port offsets (+1): 5432→5433, 6379→6380, 3000→3001 (vergate/Gitea conflicts)

---

## Environment Variables

```bash
# Rust API (veronex)
DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
VALKEY_URL=redis://localhost:6380
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_USER=veronex
CLICKHOUSE_PASSWORD=veronex
CLICKHOUSE_DB=veronex
CLICKHOUSE_ENABLED=true
OLLAMA_URL=http://localhost:11434        # legacy default; backends now stored in DB
GEMINI_API_KEY=<optional legacy>         # per-backend keys stored in DB
BOOTSTRAP_API_KEY=veronex-bootstrap-admin-key
PORT=3000                                # container internal port
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317  # optional

# Next.js web (veronex-web)
NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
NEXT_PUBLIC_VERONEX_ADMIN_KEY=veronex-bootstrap-admin-key
```

---

## Valkey Key Patterns

```
veronex:queue:jobs                              # inference job queue (RPUSH/BLPOP)
veronex:ratelimit:rpm:{key_id}:{minute}         # API key RPM sorted set
veronex:ratelimit:tpm:{key_id}:{minute}         # API key TPM counter
veronex:gemini:rpm:{backend_id}:{model}:{min}   # Gemini per-backend RPM
veronex:gemini:rpd:{backend_id}:{model}:{date}  # Gemini per-backend RPD
veronex:gemini:models:{backend_id}              # Gemini model list cache (TTL 1h)
```

> `veronex:hw:busy:{backend_id}` — in-memory `HashSet<Uuid>` (NOT Valkey)

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
    pub use_case:                  Arc<dyn InferenceUseCase>,
    pub api_key_repo:              Arc<dyn ApiKeyRepository>,
    pub backend_registry:          Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry:       Arc<dyn GpuServerRegistry>,
    pub ollama_model_repo:         Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo:      Arc<dyn OllamaSyncJobRepository>,
    pub gemini_policy_repo:        Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo:   Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo:         Arc<dyn GeminiModelRepository>,
    pub model_selection_repo:      Arc<dyn BackendModelSelectionRepository>,
    pub valkey_pool:               Option<fred::clients::RedisPool>,
    pub clickhouse_client:         Option<clickhouse::Client>,
    pub pg_pool:                   sqlx::PgPool,
}
```

---

## Helm Chart (helm/inferq/)

```
helm/inferq/
├── Chart.yaml
├── values.yaml
└── templates/
    ├── inferq/          Deployment + Service (veronex binary)
    ├── inferq-web/      Deployment + Service (Next.js)
    ├── postgres/        Deployment + Service + PVC
    ├── valkey/          Deployment + Service + PVC
    ├── clickhouse/      Deployment + Service + PVC
    ├── redpanda/        Deployment + Service
    └── otel-collector/  Deployment + Service + ConfigMap
```

→ See `docs/llm/backend/infrastructure-otel.md` for OTel pipeline details.
