# Infrastructure -- Services, Ports & Env Vars

> SSOT | **Last Updated**: 2026-04-11 (rev11: embed health probe added to health_checker + service health API)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new service | `docker-compose.yml` | New service block |
| Add new env var | `main.rs` + `docker-compose.yml` + `.env.example` | Read in `main()`, set in compose, document here |
| Add new Valkey key pattern | This file + relevant handler | Add to Valkey Key Patterns table, use `veronex:` prefix |
| Modify DB schema | `docker/postgres/init.sql` | Edit consolidated init file directly |
| Add new repo to AppState | `state.rs` + `main.rs` | Add `Arc<dyn Trait>` field, init in composition root |
| Change host port mapping | `docker-compose.yml` `ports:` | Offset convention: +1 from standard (5432->5433, 6379->6380) |
| Add Helm values | `deploy/helm/veronex/values.yaml` | Add under relevant service block; update deployment template |

## Key Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | Local dev all-in-one |
| `crates/veronex/src/main.rs` | Composition root (all adapters wired) |
| `crates/veronex/src/infrastructure/inbound/http/state.rs` | `AppState` struct |
| `docker/postgres/init.sql` | Postgres schema (consolidated, single file) |
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
| garage | dxflrs/garage:v1.0.1 | **3900** (S3 API), **3902** (web/anonymous), **3903** (admin) | S3-compatible object store (replaces MinIO; AGPLv3, Rust) |
| garage-init | dxflrs/garage (init container) | -- | Applies single-node layout, creates `veronex-messages` + `veronex-images` buckets, imports the `S3_ACCESS_KEY`/`S3_SECRET_KEY` pair, allows anonymous web GET on images bucket. Runs once on startup |
| veronex | local build | **3001**->3000 | Rust API server |
| veronex-analytics | local build | internal 3003 | Analytics (OTel write + ClickHouse read) |
| veronex-web | local build | **3000** | Next.js admin dashboard |
| veronex-agent | local build | 9091 (health) | OTLP push collector (node-exporter + Ollama → OTel Collector) |
| veronex-mcp | local build | **3100** | MCP tool server (multi-tool, single deployment) |
| veronex-embed | local build | **3200** | Embedding server |
| otel-collector | docker/otel/Dockerfile | 4317, 4318, 13133 | Metrics + traces + logs -> Redpanda |

> Port offsets (+1): 5432->5433, 6379->6380, 3000->3001 (vergate/Gitea conflicts)
> Image registry: `gitea.girok.dev/beegy-labs/*` (veronex, veronex-analytics, veronex-agent, veronex-web)

---

## Environment Variables

```bash
# Rust API (veronex)
DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
VALKEY_URL=redis://localhost:6380/0   # DB index recommended when sharing Valkey
OLLAMA_URL=http://localhost:11434
GEMINI_API_KEY=<optional legacy>
PORT=3000
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
JWT_SECRET=change-me-in-production
GEMINI_ENCRYPTION_KEY=<64-char hex>  # REQUIRED (≥32 chars; 256-bit recommended) — encrypt Gemini API keys at rest; generate: openssl rand -hex 32
# BOOTSTRAP_SUPER_USER=<username>     # optional: pre-seed super account
# BOOTSTRAP_SUPER_PASS=<password>     # optional: omit for first-run setup flow
CORS_ALLOWED_ORIGINS=*                # prod: "https://app.example.com,https://admin.example.com"
EMBED_URL=http://localhost:3200        # veronex-embed (optional — MCP vector search disabled when unset)
S3_ENDPOINT=http://localhost:3900     # S3 (Garage) API port — Sigv4 (optional — omit to store messages in PostgreSQL only)
S3_ACCESS_KEY=veronex                 # required when S3_ENDPOINT is set
S3_SECRET_KEY=veronex123              # required when S3_ENDPOINT is set
S3_BUCKET=veronex-messages
S3_IMAGE_BUCKET=veronex-images       # separate bucket for inference job images (WebP); default: veronex-images
S3_IMAGE_PUBLIC_URL=http://localhost:3902/veronex-images  # Garage web port (anonymous GET); required when S3_ENDPOINT set
S3_REGION=us-east-1
CAPACITY_ANALYZER_OLLAMA_URL=http://localhost:11434
SESSION_GROUPING_INTERVAL_SECS=86400 # session grouping loop interval (default: 86400 = 24h)
ANALYTICS_URL=http://localhost:3003
ANALYTICS_SECRET=<shared-secret>
PG_POOL_MAX=10                       # PostgreSQL pool size (default: 10)
VALKEY_POOL_SIZE=6                   # Valkey connection pool size (default: 6)
VALKEY_KEY_PREFIX=                   # optional: key namespace prefix (default: "" = no prefix). Example: "prod:" → "prod:veronex:queue:zset"

# veronex-analytics (internal service)
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_USER=veronex
CLICKHOUSE_PASSWORD=veronex
CLICKHOUSE_DB=veronex
OTEL_HTTP_ENDPOINT=http://otel-collector:4318
ANALYTICS_SECRET=<shared-secret>
# Retention TTLs — substituted into schema.sql on first volume creation
CLICKHOUSE_RETENTION_METRICS_DAYS=14
CLICKHOUSE_RETENTION_LOGS_DAYS=7
CLICKHOUSE_RETENTION_INFERENCE_DAYS=90
CLICKHOUSE_RETENTION_AUDIT_DAYS=90
CLICKHOUSE_RETENTION_TRACES_DAYS=7
CLICKHOUSE_RETENTION_MCP_DAYS=90

# veronex-agent (OTLP push collector — no HTTP server)
VERONEX_API_URL=http://veronex:3000      # target discovery endpoint
OTEL_HTTP_ENDPOINT=http://otel-collector:4318
SCRAPE_INTERVAL_MS=60000                 # scrape cycle interval (default: 60000)
REPLICA_COUNT=1                          # total StatefulSet replicas (modulus sharding)

# Next.js web (veronex-web)
NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
NEXT_PUBLIC_VERONEX_ADMIN_KEY=veronex-bootstrap-admin-key
```

---

## Valkey Key Patterns

| Key pattern | Purpose |
|-------------|---------|
| `veronex:queue:zset` | Unified ZSET priority queue (score = now_ms - tier_bonus) |
| `veronex:queue:enqueue_at` | Side hash: job_id → enqueue_at_ms (for promote_overdue) |
| `veronex:queue:model` | Side hash: job_id → model (for demand_resync) |
| `veronex:demand:{model}` | Per-model demand counter (INCR on enqueue, DECR on dispatch/cancel) |
| `veronex:queue:processing` | Processing list (legacy Phase 2; maintained for reaper crash recovery only) |
| `veronex:queue:active` | Active-processing ZSET (score = lease deadline unix_ms; Phase 3) |
| `veronex:queue:active:attempts` | Hash: job_id → re-enqueue attempt count |
| `veronex:queue:jobs:paid` | (legacy, unused after Phase 3) |
| `veronex:queue:jobs` | (legacy, unused after Phase 3) |
| `veronex:queue:jobs:test` | (legacy, unused after Phase 3) |
| `veronex:ratelimit:rpm:{key_id}` | API key RPM sorted set (sliding window) |
| `veronex:ratelimit:tpm:{key_id}:{minute}` | API key TPM counter |
| `veronex:gemini:rpm:{provider_id}:{model}:{minute}` | Gemini per-provider RPM |
| `veronex:gemini:rpd:{provider_id}:{model}:{date}` | Gemini per-provider RPD |
| `veronex:models:{provider_id}` | Provider model list cache |
| `veronex:revoked:{jti}` | JWT revocation blocklist |
| `veronex:pwreset:{token}` | Password-reset token (TTL 24h) |
| `veronex:refresh_used:{hash}` | Refresh token replay prevention |
| `veronex:login_attempts:{ip}` | IP-based login attempt counter (5-min window) |
| `veronex:throttle:{provider_id}` | Thermal Hard throttle (TTL 360s) |
| `veronex:hw:{provider_id}` | hw_metrics JSON (TTL ~60s) |
| `veronex:heartbeat:{instance_id}` | Instance heartbeat (EX 30s, refreshed every 10s) |
| `veronex:svc:health:{instance_id}` | Per-pod infra health HASH (postgresql/valkey/clickhouse/s3/vespa/embed → `{s,ms,t}` JSON, TTL 60s) |
| `veronex:slots:{provider_id}:{model}` | Distributed slot counts HASH (`{instance_id}` → count, `__max__` → max) |
| `veronex:slot_leases:{provider_id}:{model}` | Slot lease ZSET for crash recovery (score = expiry ts) |
| `veronex:job:owner:{job_id}` | Job ownership key (EX 300s) |
| `veronex:stream:tokens:{job_id}` | Cross-instance token relay (Valkey Streams) |
| `veronex:pubsub:job_events` | Pub/sub channel for job status events |
| `veronex:pubsub:cancel:{job_id}` | Pub/sub channel for cancellation signals |
| `veronex:pubsub:cancel:*` | PSUBSCRIBE pattern for all cancel channels |

> SSOT for all key patterns: `crates/veronex/src/infrastructure/outbound/valkey_keys.rs`
> All patterns above assume no prefix (`VALKEY_KEY_PREFIX=""`). When a prefix is set, it is prepended to every key (e.g. `"prod:veronex:queue:zset"`).

---

## DB Schema (`docker/postgres/init.sql`)

Single consolidated file — no migration framework.

| Table | Description |
|-------|-------------|
| `api_keys` | Bearer tokens with RPM/TPM rate limits and per-key usage tracking |
| `inference_jobs` | Job lifecycle: `provider_type`, `provider_id`, `messages_json`, `image_keys` (TEXT[]) |
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

> Exception: `roles` and `mcp_loop_tool_calls` tables use `gen_random_uuid()` (non-time-ordered PKs).

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
| Infra | `message_store` (Option, S3), `image_store` (Option, S3), `valkey_pool` (Option), `pg_pool` |

> Full port catalog with adapter mappings: `docs/llm/policies/architecture.md` -- Port Catalog.

