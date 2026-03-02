# Veronex Runbook

> On-call operational reference for the Veronex LLM inference gateway.
> Last updated: 2026-03-02

---

## Table of Contents

1. [Service Overview](#1-service-overview)
2. [Deployment](#2-deployment)
3. [Configuration](#3-configuration)
4. [Backup & Recovery](#4-backup--recovery)
5. [Scaling](#5-scaling)
6. [Incident Response](#6-incident-response)
7. [Database Maintenance](#7-database-maintenance)
8. [Health Checks](#8-health-checks)
9. [Log Locations](#9-log-locations)
10. [Bootstrap / First Run](#10-bootstrap--first-run)

---

## 1. Service Overview

### Architecture Diagram

```
                          ┌─────────────────────────────────────────────────────┐
                          │                  Veronex Cluster                    │
                          │                                                     │
  User / Client           │   ┌──────────┐     ┌──────────┐    ┌────────────┐  │
  ──────────────  HTTPS ──┼──▶│  Web UI  │     │ Veronex  │───▶│ PostgreSQL │  │
                          │   │ Next.js  │     │   API    │    │  port 5432 │  │
                          │   │ :3002    │     │  :3000   │    │  (h:5433)  │  │
                          │   └──────────┘     └────┬─────┘    └────────────┘  │
  API Consumer            │                         │                           │
  ──────────────  HTTPS ──┼─────────────────────────┤                           │
                          │                         │          ┌────────────┐  │
                          │                         ├─────────▶│   Valkey   │  │
                          │                         │          │  :6379     │  │
                          │                         │          │  (h:6380)  │  │
                          │                         │          └────────────┘  │
                          │                         │                           │
                          │                    ┌────▼─────┐                    │
                          │                    │  Queue   │  (Valkey BLPOP)    │
                          │                    └────┬─────┘                    │
                          │                         │                           │
                          │              ┌──────────┼──────────┐               │
                          │              │          │          │               │
                          │         ┌────▼───┐ ┌───▼────┐  ┌──▼──────┐       │
                          │         │ Ollama │ │ Gemini │  │ (future)│       │
                          │         │ local  │ │  API   │  │  vLLM   │       │
                          │         └────────┘ └────────┘  └─────────┘       │
                          │                                                     │
                          │   ┌───────────────────────────────────────────┐    │
                          │   │             Observability Stack            │    │
                          │   │                                           │    │
                          │   │  Veronex ──▶ veronex-analytics (:3003)   │    │
                          │   │                    │                      │    │
                          │   │             OTel Collector                │    │
                          │   │            (:4317 gRPC / :4318 HTTP)      │    │
                          │   │                    │                      │    │
                          │   │             Redpanda (:9092)              │    │
                          │   │                    │                      │    │
                          │   │       ClickHouse (:8123 HTTP / :9000 TCP) │    │
                          │   └───────────────────────────────────────────┘    │
                          └─────────────────────────────────────────────────────┘
```

### Service Dependencies

| Service              | Port (host) | Port (container) | Role                                     | Required |
|----------------------|-------------|------------------|------------------------------------------|----------|
| veronex              | 3001        | 3000             | Core API gateway                         | Yes      |
| web (Next.js)        | 3002        | 3000             | Dashboard UI                             | No       |
| PostgreSQL 18        | 5433        | 5432             | Primary datastore                        | Yes      |
| Valkey               | 6380        | 6379             | Job queue, JWT revocation, rate limits   | Yes      |
| Ollama               | 11434       | 11434            | Local LLM backend                        | No*      |
| veronex-analytics    | 3003        | 3003             | OTel ingest + ClickHouse reads           | No**     |
| OTel Collector       | 4317/4318   | 4317/4318        | Telemetry routing                        | No**     |
| Redpanda             | 9092        | 9092             | Analytics event stream                   | No**     |
| ClickHouse           | 8123/9000   | 8123/9000        | Analytics storage                        | No**     |

\* At least one LLM backend (Ollama or Gemini) must be configured for inference to work.
\** Fail-open: inference continues if analytics stack is unavailable; events are lost.

---

## 2. Deployment

### Local Development

```bash
# Start all services
docker compose up -d

# View running containers
docker compose ps

# Tail logs for all services
docker compose logs -f

# Stop everything
docker compose down
```

The `web` service depends on `NEXT_PUBLIC_VERONEX_API_URL` and `NEXT_PUBLIC_VERONEX_ADMIN_KEY` being set in `.env.local` or the compose environment.

---

## 3. Configuration

### Required Environment Variables

| Variable                        | Description                                            | Example                                               |
|---------------------------------|--------------------------------------------------------|-------------------------------------------------------|
| `DATABASE_URL`                  | PostgreSQL connection string                           | `postgres://veronex:veronex@localhost:5433/veronex`  |
| `VALKEY_URL`                    | Valkey (Redis-compatible) connection string            | `redis://localhost:6380`                              |
| `JWT_SECRET`                    | Secret for signing JWT tokens (min 32 chars)           | `<random-256-bit-hex>`                                |
| `OLLAMA_URL`                    | Ollama backend base URL                                | `http://localhost:11434`                              |
| `ANALYTICS_URL`                 | veronex-analytics internal URL                         | `http://localhost:3003`                               |
| `ANALYTICS_SECRET`              | Shared secret for analytics ingestion                  | `<random-secret>`                                     |
| `GEMINI_API_KEY`                | Google Gemini API key (optional)                       | `AIza...`                                             |
| `CLICKHOUSE_URL`                | ClickHouse HTTP URL                                    | `http://localhost:8123`                               |
| `CLICKHOUSE_USER`               | ClickHouse username                                    | `veronex`                                             |
| `CLICKHOUSE_PASSWORD`           | ClickHouse password                                    | `<password>`                                          |
| `CLICKHOUSE_DB`                 | ClickHouse database name                               | `veronex`                                             |
| `OTEL_EXPORTER_OTLP_ENDPOINT`  | OTel Collector gRPC endpoint                           | `http://otel-collector:4317`                          |
| `CAPACITY_ANALYZER_OLLAMA_URL` | Ollama URL for capacity analysis (default: OLLAMA_URL) | `http://localhost:11434`                              |
| `OLLAMA_NUM_PARALLEL`           | Max concurrent Ollama requests per model               | `1`                                                   |
| `NEXT_PUBLIC_VERONEX_API_URL`  | Web UI: backend API URL                                | `http://localhost:3001`                               |
| `NEXT_PUBLIC_VERONEX_ADMIN_KEY`| Web UI: bootstrap admin key                            | `veronex-bootstrap-admin-key`                         |

### How to Rotate JWT_SECRET

> Veronex uses a single `JWT_SECRET`. Rotation invalidates all active sessions — schedule during low-traffic.

```bash
# 1. Generate a new secret
openssl rand -hex 32

# 2. Update JWT_SECRET in .env

# 3. Restart to apply
docker compose restart veronex

# 4. Verify: existing sessions will fail (expected); users re-login
```

### How to Add a Gemini API Key

```bash
# Option A — via environment variable (applies to all Gemini backends)
# Set GEMINI_API_KEY in your secrets manager and redeploy.

# Option B — via API (per-backend key, stored encrypted in PostgreSQL)
curl -X POST http://localhost:3001/v1/backends \
  -H "Authorization: Bearer <admin-jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "gemini-pro",
    "backend_type": "gemini",
    "model": "gemini-1.5-pro",
    "api_key": "AIza..."
  }'

# The API key is encrypted at rest in the `llm_backends.api_key_encrypted` column.
# To update the key on an existing backend without changing other settings:
curl -X PATCH http://localhost:3001/v1/backends/<backend-id> \
  -H "Authorization: Bearer <admin-jwt>" \
  -H "Content-Type: application/json" \
  -d '{"api_key": "AIza-new-key"}'
```

---

## 4. Backup & Recovery

Backup은 프로젝트 외부(호스트 cron, 스토리지 스냅샷 등)에서 관리합니다. 필요할 때 아래 명령으로 수동 덤프 가능합니다.

### Manual pg_dump

```bash
# One-off dump to current directory
docker compose exec postgres pg_dump \
  -U veronex -Fc veronex > veronex_$(date +%Y%m%d_%H%M%S).dump
```

### Restore from Dump

```bash
# 1. Stop veronex to prevent writes during restore
docker compose stop veronex

# 2. Drop and recreate the database
docker compose exec postgres psql -U veronex -c "DROP DATABASE IF EXISTS veronex;"
docker compose exec postgres psql -U veronex -c "CREATE DATABASE veronex;"

# 3. Restore (copy dump file into container first)
docker cp veronex_20260302.dump $(docker compose ps -q postgres):/tmp/
docker compose exec postgres pg_restore -U veronex -d veronex --no-owner /tmp/veronex_20260302.dump

# 4. Restart veronex
docker compose start veronex
```

### Valkey Data — What Is Lost If Valkey Dies

Valkey uses `appendonly.aof` for persistence. On restart, data is replayed from the AOF log.

If Valkey is lost entirely (volume deletion), the following state is lost:

| Key Pattern                      | Purpose                        | Impact of Loss                                     |
|----------------------------------|--------------------------------|----------------------------------------------------|
| `veronex:queue:jobs`             | Job queue (production)         | In-flight jobs lost; clients will timeout/retry    |
| `veronex:queue:jobs:test`        | Job queue (test mode)          | In-flight test jobs lost                           |
| `veronex:revoked:{jti}`          | JWT revocation blocklist       | Revoked tokens briefly valid until natural expiry  |
| `veronex:ratelimit:*`            | Per-key rate limit counters    | Rate limit windows reset (clients get fresh quota) |

**Recovery after Valkey loss:**

```bash
# 1. Restart Valkey — AOF replay is automatic if volume is intact
docker compose restart valkey

# 2. If volume is lost, start fresh (state rebuilds naturally)
docker compose up -d valkey

# 3. Monitor: any in-flight jobs will fail with client errors; they can be resubmitted
# 4. Note: revoked JWTs (logged-out sessions) will be valid again until their exp claim expires
#    Communicate to security team if this is a concern; force-expiring user sessions
#    requires deleting sessions from the account_sessions table in PostgreSQL:
docker compose exec postgres psql -U veronex -d veronex \
  -c "UPDATE account_sessions SET revoked_at = NOW() WHERE revoked_at IS NULL;"
```

---

## 5. Scaling

### How to Add an Ollama Backend

```bash
# Register a new Ollama instance via the API
curl -X POST http://localhost:3001/v1/backends \
  -H "Authorization: Bearer <admin-jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "ollama-node-2",
    "backend_type": "ollama",
    "base_url": "http://ollama-node-2:11434",
    "model": "qwen2.5:3b"
  }'
# Returns: { "id": "<uuid>", "name": "ollama-node-2", ... }

# Verify it appears in the backend list
curl http://localhost:3001/v1/backends \
  -H "Authorization: Bearer <admin-jwt>"

# Trigger a capacity sync to populate VRAM slots for the new backend
curl -X POST http://localhost:3001/v1/dashboard/capacity/sync \
  -H "Authorization: Bearer <admin-jwt>"
```

After adding, the Overview dashboard should show the new backend within 30 seconds (capacity analyzer runs on a 30s tick).

### How to Tune VRAM Slots

The capacity analyzer automatically computes recommended slots using the KV cache formula. To override manually:

```bash
# View current capacity settings
curl http://localhost:3001/v1/dashboard/capacity/settings \
  -H "Authorization: Bearer <admin-jwt>"

# Update settings (e.g., disable auto-analyzer, set batch interval)
curl -X PATCH http://localhost:3001/v1/dashboard/capacity/settings \
  -H "Authorization: Bearer <admin-jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "batch_enabled": true,
    "batch_interval_secs": 30,
    "analyzer_model": "qwen2.5:3b"
  }'

# View computed capacity per backend/model
curl http://localhost:3001/v1/dashboard/capacity \
  -H "Authorization: Bearer <admin-jwt>"
```

Slots are stored in the `model_capacity` table and clamped to `(1, OLLAMA_NUM_PARALLEL)`.

### When to Increase OLLAMA_NUM_PARALLEL

**Default is `1` — do not increase on AMD APU hardware.**

| Scenario                                                 | Recommendation                             |
|----------------------------------------------------------|--------------------------------------------|
| AMD Ryzen AI Max+ 395 (iGPU, GTT/VRAM misreporting bug) | Keep `OLLAMA_NUM_PARALLEL=1` (default)     |
| Dedicated NVIDIA/AMD GPU with accurate VRAM reporting    | Increase to `2`–`4` based on VRAM capacity |
| Multiple models loaded (`OLLAMA_MAX_LOADED_MODELS=2`)    | Each model queues independently — safe     |
| Throughput saturation at batch=1 on AMD APU              | Bandwidth is saturated; parallelism hurts  |

To change:

```bash
# In docker-compose.yml:
environment:
  OLLAMA_NUM_PARALLEL: "2"

# Then restart Ollama and trigger capacity sync
docker compose restart ollama
curl -X POST http://localhost:3001/v1/dashboard/capacity/sync \
  -H "Authorization: Bearer <admin-jwt>"
```

---

## 6. Incident Response

### 6.1 Veronex API Not Responding

**Diagnose:**

```bash
# Check container status
docker compose ps veronex

# View recent logs (last 100 lines)
docker logs veronex --tail 100

# Look for panic, OOM, or connection errors
docker logs veronex 2>&1 | grep -E "FATAL|panic|OOM|error"

# Check if the health endpoint is reachable
curl -f http://localhost:3001/health || echo "UNREACHABLE"
```

**Common causes and fixes:**

| Cause                        | Symptoms in logs                         | Fix                                                             |
|------------------------------|------------------------------------------|-----------------------------------------------------------------|
| DB connection failure         | `connection refused` / `SQLSTATE`        | Check `DATABASE_URL`; verify postgres is up                     |
| OOM (out of memory)           | Container exits with code 137            | Increase container memory limit; check for memory leaks         |
| Misconfigured `DATABASE_URL`  | `invalid connection string`              | Correct env var in secrets / compose file                       |
| Port conflict                 | `address already in use`                 | Find and stop the conflicting process on port 3000/3001         |
| Valkey unreachable            | `connection refused` on startup          | Start Valkey first: `docker compose up -d valkey`               |

**Fix:**

```bash
# Restart the service
docker compose restart veronex

# If DB was the issue, verify connectivity first
docker compose exec postgres pg_isready -U veronex

# If Valkey was the issue
docker compose restart valkey
docker compose restart veronex

# Watch logs after restart
docker logs veronex -f
```

---

### 6.2 High Error Rate (>10% of requests failing)

**Diagnose:**

```bash
# Check recent failed jobs via API
curl "http://localhost:3001/v1/dashboard/jobs?status=failed&limit=20" \
  -H "Authorization: Bearer <admin-jwt>" | jq '.jobs[] | {id, model_name, backend, error}'

# Or check via the web UI: /jobs page, filter by status=failed

# Check backend health
curl http://localhost:3001/v1/backends \
  -H "Authorization: Bearer <admin-jwt>" | jq '.[] | {name, status, last_error}'

# Check Ollama directly
curl http://localhost:11434/api/ps

# Check capacity/VRAM
curl http://localhost:3001/v1/dashboard/capacity \
  -H "Authorization: Bearer <admin-jwt>"
```

**Common causes and fixes:**

| Cause                        | Symptoms                                          | Fix                                                               |
|------------------------------|---------------------------------------------------|-------------------------------------------------------------------|
| Ollama process crashed        | `connection refused` on backend health check      | `docker compose restart ollama`; check Ollama logs               |
| VRAM exhaustion               | Requests fail with OOM in Ollama logs             | Reduce concurrent slots; wait for in-flight jobs to finish        |
| Model not loaded              | `model not found` error in job details            | Pull model: `ollama pull <model>` on the Ollama host              |
| Gemini API key expired        | `401 Unauthorized` in failed job details          | Rotate key: `PATCH /v1/backends/<id>` with new `api_key`         |
| num_ctx mismatch              | Context length errors, retry storms               | Verify `OLLAMA_CONTEXT_LENGTH` is set; trigger capacity sync      |

**Fix:**

```bash
# Trigger capacity resync (re-evaluates VRAM slots)
curl -X POST http://localhost:3001/v1/dashboard/capacity/sync \
  -H "Authorization: Bearer <admin-jwt>"

# Pull a missing model on the Ollama host
docker compose exec ollama ollama pull qwen2.5:3b

# Restart a stuck Ollama instance
docker compose restart ollama
```

---

### 6.3 Queue Depth High (>100 jobs backed up)

**Diagnose:**

```bash
# Check current queue depth
curl http://localhost:3001/v1/dashboard/queue/depth \
  -H "Authorization: Bearer <admin-jwt>"
# Response: { "total": 143, "standard": 140, "test": 3 }

# Check thermal status (Overview dashboard or API)
curl http://localhost:3001/v1/dashboard \
  -H "Authorization: Bearer <admin-jwt>" | jq '.thermal'

# Check if backends are processing jobs
curl http://localhost:3001/v1/backends \
  -H "Authorization: Bearer <admin-jwt>" | jq '.[] | {name, status}'
```

**Common causes and fixes:**

| Cause                        | Symptoms                                         | Fix                                                                   |
|------------------------------|--------------------------------------------------|-----------------------------------------------------------------------|
| All backends offline          | Backends show `unreachable`/`error` status       | Restore at least one backend; see 6.2                                 |
| Thermal throttle (Hard)       | Thermal level `critical` in dashboard            | Wait for cooldown (60s hysteresis); reduce load; check cooling        |
| Concurrency slots exhausted   | Slots all occupied; new jobs queuing             | Add more backends or increase slots via capacity settings             |
| Traffic spike                 | Sudden queue growth, backends healthy            | Add another Ollama node (see Section 5)                               |

**Fix:**

```bash
# Check and resolve thermal throttle
# Thermal levels: Normal(<78C) / Soft(>=85C) / Hard(>=92C)
# View in Overview dashboard at http://localhost:3002/overview

# Add concurrency (if hardware supports it)
curl -X PATCH http://localhost:3001/v1/dashboard/capacity/settings \
  -H "Authorization: Bearer <admin-jwt>" \
  -H "Content-Type: application/json" \
  -d '{"batch_enabled": true}'

# Add a new backend to increase throughput (see Section 5)

# If queue is stale/stuck, check Valkey directly
docker compose exec valkey redis-cli LLEN veronex:queue:jobs
docker compose exec valkey redis-cli LLEN veronex:queue:jobs:test
```

---

### 6.4 PostgreSQL Unreachable

**Diagnose:**

```bash
# Check container status
docker compose ps postgres

# Check if postgres is ready to accept connections
docker compose exec postgres pg_isready -U veronex

# View postgres logs
docker logs postgres --tail 50

# Check disk space (common cause of postgres crashes)
df -h
docker system df
```

**Fix:**

```bash
# Restart postgres
docker compose restart postgres

# Wait for it to be ready (may take 10-30s on large WAL replay)
until docker compose exec postgres pg_isready -U veronex; do sleep 2; done
echo "Postgres is ready"

# Restart veronex (it will have lost its connection pool)
docker compose restart veronex

# If postgres won't start (data corruption), restore from backup:
# See Section 4 — Restore from Backup
```

---

### 6.5 JWT Revocation Lag

**When:** A user logs out but their session token is still accepted for a brief period.

**Cause:** JWT revocation is implemented via Valkey TTL keys (`veronex:revoked:{jti}`). If Valkey is unavailable, the revocation check is skipped (fail-open for availability). The token remains valid until its `exp` claim expires naturally.

**Diagnose:**

```bash
# Check if a specific jti is in the revocation list
docker compose exec valkey redis-cli GET "veronex:revoked:<jti>"
# Returns the expiry timestamp if revoked, or nil if not found

# Check Valkey connectivity
docker compose exec valkey redis-cli PING
```

**Fix options:**

| Option                      | Impact                                          | When to use                                    |
|-----------------------------|-------------------------------------------------|------------------------------------------------|
| Wait for TTL expiry         | None (automatic)                                | Token lifetime is short (minutes); not urgent  |
| Restart Valkey              | Loses all rate-limit state and revocation list  | Emergency only — all rate limits reset         |
| Revoke all sessions in DB   | All users forced to re-login                    | Security incident; token must be invalidated immediately |

```bash
# Emergency: invalidate all active sessions in PostgreSQL
docker compose exec postgres psql -U veronex -d veronex \
  -c "UPDATE account_sessions SET revoked_at = NOW() WHERE revoked_at IS NULL;"

# This does not immediately block tokens (Valkey TTL is the enforcement layer),
# but prevents session refresh and forces re-login on next token expiry.

# If Valkey restart is required (loses rate-limit state):
docker compose restart valkey
```

---

## 7. Database Maintenance

### Run Migrations Manually

```bash
# Using sqlx CLI (requires DATABASE_URL env var)
export DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
sqlx migrate run --source crates/inferq/migrations/

# Check current migration status
sqlx migrate info --source crates/inferq/migrations/

# Revert last migration (use with caution in production)
sqlx migrate revert --source crates/inferq/migrations/
```

### Check Migration Status

```bash
export DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
sqlx migrate info --source crates/inferq/migrations/

# Output shows: version | description | installed_on | checksum
# Pending migrations show as "pending" in the installed_on column
```

### Add an Index for Slow Queries

```bash
# Connect to postgres
docker compose exec postgres psql -U veronex -d veronex

# Identify slow queries (requires pg_stat_statements extension)
SELECT query, calls, mean_exec_time, total_exec_time
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 20;

# Add an index (example: speed up job listing by status + created_at)
CREATE INDEX CONCURRENTLY IF NOT EXISTS
  idx_inference_jobs_status_created
  ON inference_jobs (status, created_at DESC);

# Check index usage after load
SELECT indexrelname, idx_scan, idx_tup_read
FROM pg_stat_user_indexes
WHERE relname = 'inference_jobs'
ORDER BY idx_scan DESC;
```

**Note:** Use `CONCURRENTLY` to avoid locking the table during index creation in production.

### Routine Maintenance

```bash
# Manual VACUUM ANALYZE (normally handled by autovacuum)
docker compose exec postgres psql -U veronex -d veronex \
  -c "VACUUM ANALYZE inference_jobs;"

# Check table bloat
docker compose exec postgres psql -U veronex -d veronex \
  -c "SELECT relname, n_dead_tup, n_live_tup FROM pg_stat_user_tables ORDER BY n_dead_tup DESC LIMIT 10;"

# Check database size
docker compose exec postgres psql -U veronex -d veronex \
  -c "SELECT pg_size_pretty(pg_database_size('veronex'));"
```

---

## 8. Health Checks

### Veronex API

```bash
# Basic health check
curl -f http://localhost:3001/health
# Expected: 200 OK, { "status": "ok" }

# Authenticated health (verifies DB + Valkey connectivity)
curl http://localhost:3001/v1/dashboard \
  -H "Authorization: Bearer <admin-jwt>" | jq '.status'
```

### All Services

```bash
# Show status of all compose services
docker compose ps

# Expected output: all services show "running" (not "exited" or "restarting")

# Quick connectivity test for each service
docker compose exec postgres pg_isready -U veronex   # postgres
docker compose exec valkey redis-cli PING            # valkey (expect: PONG)
curl -f http://localhost:11434/api/tags              # ollama
curl -f http://localhost:3003/health                 # veronex-analytics
```

---

## 9. Log Locations

### Veronex API Logs

Veronex emits structured JSON logs to stdout.

```bash
# Tail live logs
docker logs veronex -f

# Last N lines
docker logs veronex --tail 200

# Filter errors only
docker logs veronex 2>&1 | grep -i '"level":"error"'

# Filter by a specific request ID or job ID
docker logs veronex 2>&1 | grep '"job_id":"<uuid>"'
```

### Web UI Logs

```bash
docker logs veronex-web --tail 100 -f
```

### Analytics / ClickHouse Query

Inference events and audit logs are stored in `otel_logs` in ClickHouse.

```bash
# Connect to ClickHouse
docker compose exec clickhouse clickhouse-client \
  --user veronex --password <password> --database veronex

# Query recent inference events
SELECT
  toDateTime(Timestamp) AS ts,
  LogAttributes['event.name'] AS event,
  LogAttributes['job.id'] AS job_id,
  LogAttributes['model.name'] AS model,
  LogAttributes['latency_ms'] AS latency_ms
FROM otel_logs
WHERE LogAttributes['event.name'] = 'inference.completed'
ORDER BY Timestamp DESC
LIMIT 100;

# Query recent errors
SELECT
  toDateTime(Timestamp) AS ts,
  SeverityText,
  Body
FROM otel_logs
WHERE SeverityNumber >= 17  -- ERROR and above
ORDER BY Timestamp DESC
LIMIT 50;

# Query audit events
SELECT
  toDateTime(Timestamp) AS ts,
  LogAttributes['audit.action'] AS action,
  LogAttributes['account.id'] AS account
FROM otel_logs
WHERE LogAttributes['event.name'] = 'audit.action'
ORDER BY Timestamp DESC
LIMIT 50;
```

### Redpanda (Kafka) Logs

```bash
# Check Redpanda topic for otel-logs
docker compose exec redpanda rpk topic list
docker compose exec redpanda rpk topic consume otel-logs --num 10
```

---

## 10. Bootstrap / First Run

### Prerequisites

Ensure all required services are running:

```bash
docker compose up -d postgres valkey
# Wait for postgres to be ready
until docker compose exec postgres pg_isready -U veronex; do sleep 2; done
```

### Run Database Migrations

```bash
export DATABASE_URL=postgres://veronex:veronex@localhost:5433/veronex
sqlx migrate run --source crates/inferq/migrations/
```

### Start Veronex

```bash
docker compose up -d veronex
curl -f http://localhost:3001/health
```

### Create the Initial Admin Account

```bash
# Bootstrap admin account via the setup endpoint
# (Only available when no accounts exist in the database)
curl -X POST http://localhost:3001/v1/setup \
  -H "Content-Type: application/json" \
  -d '{
    "email": "admin@example.com",
    "password": "changeme-use-strong-password",
    "name": "Admin"
  }'
# Returns: { "account": { "id": "...", "email": "..." }, "token": "<jwt>" }
```

**Important:** The `/v1/setup` endpoint is disabled once any account exists. Change the default password immediately.

### Default Admin API Key

The bootstrap admin key is set via the `NEXT_PUBLIC_VERONEX_ADMIN_KEY` environment variable:

```
Default: veronex-bootstrap-admin-key
```

**Rotate this key immediately after first login:**

1. Log in to the web UI at `http://localhost:3002`
2. Navigate to API Keys
3. Create a new standard key
4. Delete or disable the bootstrap key
5. Update `NEXT_PUBLIC_VERONEX_ADMIN_KEY` in your environment

### Start Remaining Services

```bash
# Start the full stack
docker compose up -d

# Verify all services are healthy
docker compose ps

# Open the web dashboard
open http://localhost:3002
```

### Register an Ollama Backend

```bash
# Get a JWT by logging in
TOKEN=$(curl -s -X POST http://localhost:3001/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"<your-password>"}' \
  | jq -r '.token')

# Register the local Ollama instance
curl -X POST http://localhost:3001/v1/backends \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "ollama-local",
    "backend_type": "ollama",
    "base_url": "http://ollama:11434",
    "model": "qwen2.5:3b"
  }'

# Trigger initial capacity analysis
curl -X POST http://localhost:3001/v1/dashboard/capacity/sync \
  -H "Authorization: Bearer $TOKEN"
```

---

*For architecture details, see `.ai/architecture.md`. For git workflow, see `.ai/git-flow.md`.*
