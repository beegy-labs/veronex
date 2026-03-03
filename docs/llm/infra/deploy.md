# Infrastructure — Services, Ports & Env Vars

> SSOT | **Last Updated**: 2026-03-03 (rev4: CORS_ALLOWED_ORIGINS env var; Helm chart at deploy/helm/veronex/)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new service | `docker-compose.yml` | New service block |
| Add new env var | `crates/veronex/src/main.rs` + `docker-compose.yml` + `.env.example` | Read in `main()`, set in compose `environment:`, document here |
| Add new Valkey key pattern | `infrastructure.rs` (this file) + relevant handler | Add to Valkey Key Patterns table, use `veronex:` prefix |
| Add new DB migration | `crates/veronex/migrations/` new `.sql` file | Update `0000000001_init.sql` or add a new sequential migration file |
| Add new repo to AppState | `infrastructure/inbound/http/state.rs` + `crates/veronex/src/main.rs` | Add `Arc<dyn Trait>` field to `AppState`, init in `main()` composition root |
| Change host port mapping | `docker-compose.yml` `ports:` + update memory/docs | Remember offset convention: +1 from standard (5432→5433, 6379→6380) |
| Add Helm values | `deploy/helm/veronex/values.yaml` | Add under the relevant service block; update env var in deployment template |

## Key Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | Local dev all-in-one |
| `crates/veronex/src/main.rs` | Composition root (all adapters wired) |
| `crates/veronex/src/infrastructure/inbound/http/state.rs` | `AppState` struct |
| `crates/veronex/migrations/` | All DB migrations |
| `docker/clickhouse/schema.sql` | ClickHouse schema (with `__RETENTION_*__` placeholders) |
| `docker/clickhouse/init.sh` | Substitutes retention env vars and applies schema |

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

→ Full pipeline spec: `docs/llm/infra/otel-pipeline.md`

## Services

| Service | Image | Host Port | Role |
|---------|-------|-----------|------|
| postgres | postgres:18-alpine | **5433** | Main DB — PG18, native `uuidv7()` |
| valkey | valkey/valkey:9.0.3-alpine | **6380** | Queue (BLPOP), rate limiting, JWT revocation blocklist |
| clickhouse | clickhouse-server:26.1 | 8123, 9000 | Analytics read layer — `otel_logs`, `otel_metrics_gauge` |
| redpanda | redpandadata/redpanda:v25.3.9 | 9092 | Single message bus (Kafka-compatible) |
| minio | minio/minio:latest | **9010** (API), **9011** (Console) | S3-compatible object store — `messages_json` conversation contexts |
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

# CORS
CORS_ALLOWED_ORIGINS=*                   # "*" = allow any origin (default, local dev)
                                         # production: "https://app.example.com,https://admin.example.com"

# S3 / MinIO — conversation context storage (MANDATORY)
S3_ENDPOINT=http://localhost:9010       # docker: http://minio:9000
S3_ACCESS_KEY=veronex
S3_SECRET_KEY=veronex123
S3_BUCKET=veronex-messages             # bucket auto-created on startup
S3_REGION=us-east-1                    # any valid region string for MinIO

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

# ClickHouse data retention (set before first `docker compose up -d`)
# Applied by docker/clickhouse/init.sh on first volume creation only.
CLICKHOUSE_RETENTION_ANALYTICS_DAYS=90    # otel_logs (inference + audit events)
CLICKHOUSE_RETENTION_METRICS_DAYS=30     # otel_metrics_gauge, otel_traces_raw, node_metrics
CLICKHOUSE_RETENTION_AUDIT_DAYS=365      # audit_events (legacy table)

# Next.js web (veronex-web)
NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
NEXT_PUBLIC_VERONEX_ADMIN_KEY=veronex-bootstrap-admin-key
```

---

## Valkey Key Patterns

```
veronex:queue:jobs:paid                         # inference job queue — paid-tier API key requests (BLPOP polled first)
veronex:queue:jobs                              # inference job queue — standard/free-tier API key requests (BLPOP polled second)
veronex:queue:jobs:test                         # inference job queue — test run requests (BLPOP polled third)
veronex:ratelimit:rpm:{key_id}:{minute}         # API key RPM sorted set
veronex:ratelimit:tpm:{key_id}:{minute}         # API key TPM counter
veronex:gemini:rpm:{provider_id}:{model}:{min}   # Gemini per-provider RPM
veronex:gemini:rpd:{provider_id}:{model}:{date}  # Gemini per-provider RPD
veronex:gemini:models:{provider_id}              # Gemini model list cache (TTL 1h)
veronex:revoked:{jti}                            # JWT revocation blocklist (TTL = remaining token lifetime)
veronex:pwreset:{raw_token}                      # password-reset one-time token (TTL 24h)
veronex:throttle:{provider_id}                   # thermal Hard throttle flag (TTL 90s, set by health_checker)
veronex:hw:{provider_id}                         # hw_metrics JSON (temp_c, vram_used_mb, etc., TTL ~60s)
```

> Concurrency slots are **in-process** `Arc<Semaphore>` in `ConcurrencySlotMap` (NOT Valkey).

---

## DB Migrations (crates/veronex/migrations/)

Single init migration: `0000000001_init.sql` — creates all tables in one schema file.

Key tables included:

| Table | Description |
|-------|-------------|
| `api_keys` | Bearer tokens with RPM/TPM rate limits and per-key usage tracking |
| `inference_jobs` | Job lifecycle records: `provider_type`, `provider_id`, `messages_json`, etc. |
| `llm_providers` | Provider config records (Ollama/Gemini): `provider_type`, VRAM, server FK |
| `gpu_servers` | GPU hardware nodes with `node_exporter_url` |
| `gemini_rate_limit_policies` | Per-model RPM/RPD limits + `available_on_free_tier` flag |
| `provider_selected_models` | Per-provider model enable/disable toggles (`PRIMARY KEY (provider_id, model_name)`) |
| `gemini_sync_config` | Singleton admin API key for Gemini model sync |
| `gemini_models` | Global Gemini model pool (synced via admin key) |
| `ollama_models` | Per-provider model list (`PRIMARY KEY (model_name, provider_id)`) |
| `ollama_sync_jobs` | Async global sync tracking: `total_providers`, `done_providers` |
| `accounts` | RBAC accounts (super \| admin, Argon2id password_hash) |
| `account_sessions` | JWT sessions: `jti`, `refresh_token_hash` (BLAKE2b), rolling sessions |
| `model_capacity` | VRAM + throughput per `(provider_id, model_name)` |
| `capacity_settings` | Singleton (id=1): analyzer model, batch interval |
| `lab_settings` | Singleton (id=1): `gemini_function_calling` BOOLEAN |
| `model_pricing` | `(provider, model_name)` PK; Gemini 2026-03 seed rows; Ollama = no rows (always $0.00) |

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
    // Provider routing
    pub provider_registry:         Arc<dyn LlmProviderRegistry>,
    pub gpu_server_registry:       Arc<dyn GpuServerRegistry>,
    pub ollama_model_repo:         Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo:      Arc<dyn OllamaSyncJobRepository>,
    pub gemini_policy_repo:        Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo:   Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo:         Arc<dyn GeminiModelRepository>,
    pub model_selection_repo:      Arc<dyn ProviderModelSelectionRepository>,
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
    // Lab features
    pub lab_settings_repo:         Arc<dyn LabSettingsRepository>,
    // Object storage (messages_json)
    pub message_store:             Option<Arc<dyn MessageStore>>,   // None when S3_ENDPOINT unset
    // Infrastructure
    pub valkey_pool:               Option<fred::clients::Pool>,
    pub pg_pool:                   sqlx::PgPool,
}
```

---

## ClickHouse Data Retention

Configured via env vars in `docker-compose.yml`. Applied **on first volume creation** only.

| Variable | Default | Applies to |
|----------|---------|------------|
| `CLICKHOUSE_RETENTION_ANALYTICS_DAYS` | `90` | `otel_logs` (inference + audit events) |
| `CLICKHOUSE_RETENTION_METRICS_DAYS` | `30` | `otel_metrics_gauge`, `otel_traces_raw`, `node_metrics` |
| `CLICKHOUSE_RETENTION_AUDIT_DAYS` | `365` | `audit_events` (legacy table) |

**On an existing volume**, use ALTER TABLE to change TTL:

```sql
-- Change inference/audit log retention to 30 days
ALTER TABLE otel_logs MODIFY TTL toDate(Timestamp) + INTERVAL 30 DAY;

-- Change metrics retention to 14 days
ALTER TABLE otel_metrics_gauge MODIFY TTL toDate(TimeUnix) + INTERVAL 14 DAY;
ALTER TABLE node_metrics MODIFY TTL toDate(ts) + INTERVAL 14 DAY;
```

→ See `docs/llm/infra/otel-pipeline.md` for OTel pipeline details.

---

## Helm Deployment

Chart location: `deploy/helm/veronex/`

### First-time setup

```bash
# 1. Add required Helm repos
helm repo add bitnami https://charts.bitnami.com/bitnami
helm repo add redpanda https://charts.redpanda.com
helm repo update

# 2. Fetch subchart dependencies
helm dependency update deploy/helm/veronex/

# 3. Install (all subcharts enabled by default)
helm install veronex deploy/helm/veronex/ \
  --set veronex.jwt.secret="$(openssl rand -hex 32)" \
  --set veronex.cors.allowedOrigins="https://app.example.com"
```

### Subchart enable/disable

Each infrastructure component can be disabled and replaced with an external service:

```bash
# Use external PostgreSQL (e.g. RDS)
helm install veronex deploy/helm/veronex/ \
  --set postgresql.enabled=false \
  --set externalPostgresql.host=my-rds.example.com \
  --set externalPostgresql.username=veronex \
  --set externalPostgresql.password=secret \
  --set externalPostgresql.database=veronex

# Use external Valkey/Redis
  --set valkey.enabled=false \
  --set externalValkey.host=my-redis.example.com

# Use external MinIO / S3
  --set minio.enabled=false \
  --set externalMinio.endpoint=https://s3.amazonaws.com \
  --set externalMinio.accessKey=AKID... \
  --set externalMinio.secretKey=secret \
  --set externalMinio.bucket=my-veronex-bucket \
  --set externalMinio.region=us-east-1
```

### Ingress (optional)

```bash
helm install veronex deploy/helm/veronex/ \
  --set ingress.enabled=true \
  --set ingress.host=veronex.example.com \
  --set ingress.tls.enabled=true \
  --set ingress.tls.secretName=veronex-tls
```

### Subchart versions (Chart.yaml)

| Subchart | Repo | Condition key |
|----------|------|---------------|
| `postgresql` | bitnami | `postgresql.enabled` |
| `valkey` | bitnami | `valkey.enabled` |
| `clickhouse` | bitnami | `clickhouse.enabled` |
| `minio` | bitnami | `minio.enabled` |
| `redpanda` | charts.redpanda.com | `redpanda.enabled` |
