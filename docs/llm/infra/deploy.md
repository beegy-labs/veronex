# Infrastructure -- Services, Ports & Env Vars

> SSOT | **Last Updated**: 2026-03-04 (rev5: trimmed -- cross-refs to otel-pipeline.md and architecture.md)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new service | `docker-compose.yml` | New service block |
| Add new env var | `main.rs` + `docker-compose.yml` + `.env.example` | Read in `main()`, set in compose, document here |
| Add new Valkey key pattern | This file + relevant handler | Add to Valkey Key Patterns table, use `veronex:` prefix |
| Add new DB migration | `crates/veronex/migrations/` | Update `0000000001_init.sql` or add sequential file |
| Add new repo to AppState | `state.rs` + `main.rs` | Add `Arc<dyn Trait>` field, init in composition root |
| Change host port mapping | `docker-compose.yml` `ports:` | Offset convention: +1 from standard (5432->5433, 6379->6380) |
| Add Helm values | `deploy/helm/veronex/values.yaml` | Add under relevant service block; update deployment template |

## Key Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | Local dev all-in-one |
| `crates/veronex/src/main.rs` | Composition root (all adapters wired) |
| `crates/veronex/src/infrastructure/inbound/http/state.rs` | `AppState` struct |
| `crates/veronex/migrations/` | All DB migrations |
| `docker/clickhouse/schema.sql` | ClickHouse schema (`__RETENTION_*__` placeholders) |
| `docker/clickhouse/init.sh` | Substitutes retention env vars, applies schema |

---

## Services

> Data pipeline, ClickHouse Kafka chains, and data retention: `docs/llm/infra/otel-pipeline.md`.

| Service | Image | Host Port | Role |
|---------|-------|-----------|------|
| postgres | postgres:18-alpine | **5433** | Main DB (PG18, native `uuidv7()`) |
| valkey | valkey/valkey:9.0.3-alpine | **6380** | Queue (Lua priority pop), rate limiting, JWT revocation |
| clickhouse | clickhouse-server:26.1 | 8123, 9000 | Analytics read layer |
| redpanda | redpandadata/redpanda:v25.3.9 | 9092 | Single message bus (Kafka-compatible) |
| minio | minio/minio:latest | **9010**, **9011** | S3-compatible object store |
| veronex | local build | **3001**->3000 | Rust API server |
| veronex-analytics | local build | internal 3003 | Analytics (OTel write + ClickHouse read) |
| veronex-web | local build | 3002 | Next.js admin dashboard |
| otel-collector | docker/otel/Dockerfile | 4317, 4318, 13133 | Metrics + traces + logs -> Redpanda |

> Port offsets (+1): 5432->5433, 6379->6380, 3000->3001 (vergate/Gitea conflicts)

---

## Environment Variables

```bash
# Rust API (veronex)
DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
VALKEY_URL=redis://localhost:6380
OLLAMA_URL=http://localhost:11434
GEMINI_API_KEY=<optional legacy>
BOOTSTRAP_API_KEY=veronex-bootstrap-admin-key
PORT=3000
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
JWT_SECRET=change-me-in-production
# BOOTSTRAP_SUPER_USER=<username>     # optional: pre-seed super account
# BOOTSTRAP_SUPER_PASS=<password>     # optional: omit for first-run setup flow
CORS_ALLOWED_ORIGINS=*                # prod: "https://app.example.com,https://admin.example.com"
S3_ENDPOINT=http://localhost:9010     # S3/MinIO (MANDATORY)
S3_ACCESS_KEY=veronex
S3_SECRET_KEY=veronex123
S3_BUCKET=veronex-messages
S3_REGION=us-east-1
CAPACITY_ANALYZER_OLLAMA_URL=http://localhost:11434
OLLAMA_NUM_PARALLEL=1                # slot ceiling in capacity analyzer; must match Ollama StatefulSet env
SESSION_GROUPING_INTERVAL_SECS=86400 # session grouping loop interval (default: 86400 = 24h)
ANALYTICS_URL=http://localhost:3003
ANALYTICS_SECRET=<shared-secret>

# veronex-analytics (internal service)
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_USER=veronex
CLICKHOUSE_PASSWORD=veronex
CLICKHOUSE_DB=veronex
OTEL_HTTP_ENDPOINT=http://otel-collector:4318
ANALYTICS_SECRET=<shared-secret>
CLICKHOUSE_RETENTION_ANALYTICS_DAYS=90   # set before first `docker compose up`
CLICKHOUSE_RETENTION_METRICS_DAYS=30
CLICKHOUSE_RETENTION_AUDIT_DAYS=365

# Next.js web (veronex-web)
NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
NEXT_PUBLIC_VERONEX_ADMIN_KEY=veronex-bootstrap-admin-key
```

---

## Valkey Key Patterns

| Key pattern | Purpose |
|-------------|---------|
| `veronex:queue:jobs:paid` | Paid-tier job queue (Lua priority pop, tried first) |
| `veronex:queue:jobs` | Standard/free-tier job queue (polled second) |
| `veronex:queue:jobs:test` | Test run queue (polled third) |
| `veronex:queue:processing` | Processing list (BLMOVE destination for reliable queue) |
| `veronex:ratelimit:rpm:{key_id}` | API key RPM sorted set (sliding window) |
| `veronex:ratelimit:tpm:{key_id}:{minute}` | API key TPM counter |
| `veronex:gemini:rpm:{provider_id}:{model}:{minute}` | Gemini per-provider RPM |
| `veronex:gemini:rpd:{provider_id}:{model}:{date}` | Gemini per-provider RPD |
| `veronex:models:{provider_id}` | Provider model list cache |
| `veronex:revoked:{jti}` | JWT revocation blocklist |
| `veronex:pwreset:{token}` | Password-reset token (TTL 24h) |
| `veronex:refresh_used:{hash}` | Refresh token replay prevention |
| `veronex:login_attempts:{ip}` | IP-based login attempt counter (5-min window) |
| `veronex:throttle:{provider_id}` | Thermal Hard throttle (TTL 90s) |
| `veronex:hw:{provider_id}` | hw_metrics JSON (TTL ~60s) |
| `veronex:heartbeat:{instance_id}` | Instance heartbeat (EX 30s, refreshed every 10s) |
| `veronex:vram:{provider_id}` | Distributed VRAM reservation state HASH |
| `veronex:job:owner:{job_id}` | Job ownership key (EX 300s) |
| `veronex:stream:tokens:{job_id}` | Cross-instance token relay (Valkey Streams) |
| `veronex:pubsub:job_events` | Pub/sub channel for job status events |
| `veronex:pubsub:cancel:{job_id}` | Pub/sub channel for cancellation signals |

> SSOT for all key patterns: `crates/veronex/src/infrastructure/outbound/valkey_keys.rs`

---

## DB Migrations (crates/veronex/migrations/)

Single init migration: `0000000001_init.sql` -- all tables in one schema file.

| Table | Description |
|-------|-------------|
| `api_keys` | Bearer tokens with RPM/TPM rate limits and per-key usage tracking |
| `inference_jobs` | Job lifecycle: `provider_type`, `provider_id`, `messages_json` |
| `llm_providers` | Provider config (Ollama/Gemini): `provider_type`, VRAM, server FK |
| `gpu_servers` | GPU hardware nodes with `node_exporter_url` |
| `gemini_rate_limit_policies` | Per-model RPM/RPD limits + `available_on_free_tier` flag |
| `provider_selected_models` | Per-provider model enable/disable (`PK (provider_id, model_name)`) |
| `gemini_sync_config` | Singleton admin API key for Gemini model sync |
| `gemini_models` | Global Gemini model pool (synced via admin key) |
| `ollama_models` | Per-provider model list (`PK (model_name, provider_id)`) |
| `ollama_sync_jobs` | Async global sync tracking |
| `accounts` | RBAC accounts (super / admin, Argon2id password_hash) |
| `account_sessions` | JWT sessions: `jti`, `refresh_token_hash` (BLAKE2b) |
| `model_vram_profiles` | VRAM profiles per `(provider_id, model_name)` — weight, KV, arch params |
| `capacity_settings` | Singleton (id=1): analyzer model, sync interval, sync_enabled |
| `lab_settings` | Singleton (id=1): `gemini_function_calling` BOOLEAN |
| `model_pricing` | `(provider, model_name)` PK; Gemini seed rows; Ollama = $0.00 |

---

## UUID Policy

All PKs use **UUIDv7** (time-ordered, k-sortable). Rust: `Uuid::now_v7()` before INSERT. PG18: `DEFAULT uuidv7()` fallback. Never use `Uuid::new_v4()` or `gen_random_uuid()`.

---

## AppState (state.rs)

Categories of `Arc<dyn Port>` fields wired in `main.rs` composition root:

| Category | Key fields |
|----------|------------|
| Inference core | `use_case`, `job_repo`, `api_key_repo` |
| Provider routing | `provider_registry`, `gpu_server_registry`, `ollama_model_repo`, `gemini_*` repos, `model_selection_repo` |
| Auth / RBAC | `account_repo`, `session_repo`, `jwt_secret` |
| Observability | `audit_port`, `analytics_repo` |
| Capacity / thermal | `vram_pool`, `thermal`, `vram_profile_repo`, `capacity_settings_repo`, `sync_trigger`, `analyzer_url` |
| Lab features | `lab_settings_repo` |
| Infra | `message_store` (Option, S3), `valkey_pool` (Option), `pg_pool` |

> Full port catalog with adapter mappings: `docs/llm/policies/architecture.md` -- Port Catalog.

---

## Helm Deployment

Chart location: `deploy/helm/veronex/`

```bash
# First-time setup
helm repo add bitnami https://charts.bitnami.com/bitnami
helm repo add redpanda https://charts.redpanda.com
helm repo update
helm dependency update deploy/helm/veronex/

# Install (all subcharts enabled by default)
helm install veronex deploy/helm/veronex/ \
  --set veronex.jwt.secret="$(openssl rand -hex 32)" \
  --set veronex.cors.allowedOrigins="https://app.example.com"
```

Disable subcharts to use external services:

| Subchart | Disable flag | External config prefix |
|----------|-------------|------------------------|
| `postgresql` | `postgresql.enabled=false` | `externalPostgresql.{host,username,password,database}` |
| `valkey` | `valkey.enabled=false` | `externalValkey.host` |
| `minio` | `minio.enabled=false` | `externalMinio.{endpoint,accessKey,secretKey,bucket,region}` |
| `clickhouse` | `clickhouse.enabled=false` | -- |
| `redpanda` | `redpanda.enabled=false` | -- |

Ingress (optional):

```bash
helm install veronex deploy/helm/veronex/ \
  --set ingress.enabled=true \
  --set ingress.host=veronex.example.com \
  --set ingress.tls.enabled=true \
  --set ingress.tls.secretName=veronex-tls
```
