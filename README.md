# Veronex

**Autonomous intelligence scheduler/gateway for N Ollama servers** — VRAM-aware routing, adaptive concurrency learning, thermal protection, OpenAI-compatible API, Next.js admin dashboard.

---

## Vision

Veronex is not a simple reverse proxy. It is an **intelligence scheduler** that:

1. **Unified control of N servers** — treats all Ollama instances as a single compute pool
2. **Cluster-wide optimization** — maximizes total throughput across all servers, not individual server performance
3. **Dynamic model allocation** — computes optimal "model combination + concurrent request count" per server in real-time
4. **Multi-model co-residence** — when VRAM allows, loads multiple models simultaneously for parallel processing; when insufficient, FIFO + model locality to minimize switching cost
5. **3-phase adaptive learning** — Cold Start (limit=1) → AIMD (TPS+p95 per model) → LLM Batch (all-model combination tuning with ±2 clamp)
6. **Thermal protection** — auto decelerate → block → cooldown → gradual recovery (per-provider thresholds, auto-detected from GPU vendor)
7. **Self-healing** — circuit breaker per provider, crash recovery via Valkey, queue reaper for orphaned jobs

Primary optimization target: **AMD Ryzen AI 395+** (APU/iGPU Vulkan inference).

---

## Features

| Category | Details |
|----------|---------|
| **OpenAI API** | `POST /v1/chat/completions`, Ollama native, Gemini native, streaming SSE |
| **Multi-provider routing** | Ollama (local GPU) + Google Gemini; VRAM-based ranking + model stickiness |
| **VRAM pool** | Lock-free CAS-based per-provider VRAM reservation; weight + KV cache tracking; RAII permits |
| **Adaptive concurrency** | Per-model AIMD learning: TPS ratio + p95 spike detection → automatic limit adjustment |
| **LLM batch analysis** | qwen2.5:3b analyzes all loaded models → recommends optimal max_concurrent (±2 clamp) |
| **Thermal throttle** | Per-provider GPU/CPU profiles (auto-detected via sysfs vendor ID); Normal → Soft → Hard levels |
| **Priority queue** | 3-lane Valkey queue (paid / standard / test); Lua priority pop |
| **API key management** | BLAKE2b hash storage, per-key RPM/TPM rate limits, paid/free tiers, usage analytics |
| **JWT auth + RBAC** | super / admin / user roles, rolling refresh, Valkey revocation blocklist |
| **Audit trail** | All admin actions → OTel → Redpanda → ClickHouse |
| **Conversation sessions** | `X-Conversation-ID` header or daily batch grouping via Blake2b prefix-hash chain |
| **Analytics pipeline** | OTel Collector → Redpanda → ClickHouse MV; `veronex-analytics` sidecar (port 3003) |
| **Dashboard** | Next.js 16 — Overview, Usage, Performance, Jobs, API Keys, Providers, Servers, Accounts, Audit |
| **Network flow** | Real-time inference traffic visualization (ArgoCD-style SVG + live feed) |
| **i18n** | English / Korean / Japanese |
| **Docker Compose** | Single command starts all services |

---

## Capacity Learning (AIMD + LLM Batch)

Veronex learns the optimal concurrency limit per model automatically:

```
Phase 1: Cold Start
  New model → max_concurrent = 1
  First inference data → set baseline TPS + p95

Phase 2: AIMD (every sync cycle, ~300s)
  stats = compute_throughput_stats(provider, model, 1h)
  ratio = current_tps / baseline_tps

  if ratio < 0.7 OR p95 > baseline_p95 × 2:
    max_concurrent = max(1, current × 3/4)     # multiplicative decrease
  elif ratio >= 0.9:
    max_concurrent += 1                         # additive increase
    baseline = max(baseline, current_tps)

Phase 3: LLM Batch (when total_samples >= 10)
  qwen2.5:3b analyzes ALL loaded models on the provider
  → per-model recommended_max_concurrent
  → clamped to ±2 from current (stability)
  → clamped to weight-based upper bound × 2

Dispatch:
  if active_count >= max_concurrent → re-enqueue (wait in queue)
  Probe policy: periodically allows ±N above/below limit for exploration
```

---

## Quick Start

```bash
# 1. Clone
git clone <repo>
cd veronex

# 2. Configure
cp .env.example .env
# Required: set JWT_SECRET
# Optional: set OLLAMA_URL if Ollama is not on the host

# 3. Start
JWT_SECRET=<your-secret> docker compose up -d

# 4. Open dashboard
open http://localhost:3002
# Follow the setup wizard to create your admin account
```

> **macOS (Docker Desktop)**: default `OLLAMA_URL=http://host.docker.internal:11434` works out of the box.
> **Linux**: set `OLLAMA_URL=http://172.17.0.1:11434` (or your host bridge IP) in `.env`.

---

## Service Ports

| Service | Host Port | Purpose |
|---------|-----------|---------|
| veronex API | **3001** | OpenAI-compatible + admin REST API |
| veronex Web | **3002** | Next.js dashboard |
| PostgreSQL | 5433 | Primary datastore |
| Valkey | 6380 | Job queue + rate limiting + session revocation |
| ClickHouse | 8123, 9000 | Analytics store |
| Redpanda | 9092 | OTel event stream (Kafka-compatible) |
| MinIO | 9011 (console), 9010 (S3 API) | S3-compatible message storage |

---

## Deployment

### Prerequisites

- Docker 24+ with Compose v2 (`docker compose version`)
- Ollama running on the host or a reachable server

### Full Setup

```bash
# Build + start all services
JWT_SECRET=$(openssl rand -hex 32) docker compose up -d --build

# First run: visit http://localhost:3002 for the setup wizard
# Or auto-bootstrap with env vars:
BOOTSTRAP_SUPER_USER=admin BOOTSTRAP_SUPER_PASS=<pass> docker compose up -d
```

### Adding a Provider

1. Dashboard → **Providers** → Ollama → add provider URL (e.g. `http://host.docker.internal:11434`)
2. The health-checker (30s interval) will detect the provider and start routing
3. Models sync automatically; capacity learning begins on first inference

### Updating

```bash
git pull
docker compose build
docker compose up -d
# Database migrations run automatically on startup
```

### Env Variables

See `.env.example` for the full list. Minimum required:

```bash
JWT_SECRET=<openssl rand -hex 32>   # required -- JWT signing key
OLLAMA_URL=http://host.docker.internal:11434  # default (macOS)
```

---

## API Usage

```bash
# Create an API key (dashboard -> Keys -> Create Key)
# Then use it with the OpenAI SDK or curl:

curl http://localhost:3001/v1/chat/completions \
  -H "Authorization: Bearer iq_..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama3.2",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

Compatible with [OpenAI Python SDK](https://github.com/openai/openai-python):

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:3001/v1", api_key="iq_...")
response = client.chat.completions.create(model="llama3.2", messages=[...])
```

Interactive API docs available at `http://localhost:3002/api-docs`.

---

## Architecture

```
Client → POST /v1/chat/completions (X-API-Key)
          ↓ API key auth + rate limit (RPM/TPM)
          ↓ RPUSH veronex:queue:jobs:paid | :jobs | :jobs:test
          ↓ queue_dispatcher: Lua priority pop → processing list
          ↓ 2-stage model filter:
          │   1. providers_for_model() → has the model?
          │   2. list_enabled() → model enabled on this provider?
          ↓ VRAM sort + model stickiness (+100GB bonus for loaded model)
          ↓ Gate chain:
          │   circuit_breaker → thermal → concurrency limit → vram_pool.try_reserve()
          ↓ OllamaAdapter | GeminiAdapter → SSE tokens → client
          ↓ VramPermit dropped (RAII) → KV cache returned, weight stays
          ↓ ObservabilityPort → veronex-analytics → OTel → ClickHouse
```

Two Rust crates:
- **`veronex`** — API server + scheduler (`crates/veronex/`)
- **`veronex-analytics`** — OTel ingest + analytics read API (`crates/veronex-analytics/`, port 3003)

Hexagonal architecture (Ports & Adapters):
```
domain/ (entities, enums, value objects — no deps)
  ↑
application/ (use cases + port traits)
  ↑
infrastructure/ (Axum handlers, Postgres, Valkey, Ollama, Gemini, OTel adapters)
```

Full architecture details: [`.ai/architecture.md`](.ai/architecture.md)
Domain CDD docs: [`docs/llm/`](docs/llm/)

---

## Tech Stack

| Layer | Tech |
|-------|------|
| Runtime | Rust (Axum 0.8, tokio, Edition 2024) |
| DB | PostgreSQL 18 (sqlx 0.8, native uuidv7()) |
| Queue | Valkey (fred 10, Lua priority pop) |
| Streaming | SSE (Server-Sent Events) |
| Analytics | ClickHouse + OTel Collector + Redpanda |
| Web | Next.js 16, Tailwind v4, shadcn/ui |
| Deploy | Docker Compose, Kubernetes (Helm) |

---

## Branch Strategy

| Branch | Purpose |
|--------|---------|
| `main` | Stable production releases |
| `develop` | Active development |
| `feat/*` | Feature branches |

---

## License

MIT
