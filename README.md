# Veronex

**OpenAI-compatible LLM inference gateway** — queue-based, multi-backend, VRAM-aware — with a Next.js monitoring dashboard.

---

## Features

| Category | Details |
|----------|---------|
| **OpenAI API** | `POST /v1/chat/completions`, Ollama native, Gemini native, streaming SSE |
| **Multi-backend routing** | Ollama (local) + Google Gemini; VRAM-based ranking; hot-failover |
| **Dynamic concurrency** | Per `(backend, model)` semaphore — capacity auto-tuned from VRAM and KV cache formula |
| **Thermal throttle** | Soft ≥ 85 °C / Hard ≥ 92 °C with 60 s cooldown; node-exporter + AMD APU support |
| **Priority queue** | 3-lane Valkey queue (paid / standard / test); BLPOP strict priority |
| **API key management** | BLAKE2b hash storage, per-key RPM/TPM rate limits, paid/free tiers, usage analytics |
| **JWT auth + RBAC** | super / admin / user roles, rolling refresh, Valkey revocation blocklist |
| **Audit trail** | All key admin actions → OTel → Redpanda → ClickHouse |
| **Conversation sessions** | `X-Conversation-ID` header or daily batch grouping via Blake2b prefix-hash chain |
| **Analytics pipeline** | OTel Collector → Redpanda → ClickHouse MV; `veronex-analytics` sidecar (port 3003) |
| **Dashboard** | Next.js 15 — Overview, Usage, Performance, Jobs, API Keys, Providers, Servers, Accounts, Audit |
| **Network flow** | Real-time inference traffic visualization (ArgoCD-style SVG + live feed) in Jobs page |
| **i18n** | English / Korean / Japanese |
| **Docker Compose** | Single command starts all services |

---

## Quick Start

```bash
# 1. Clone
git clone <repo>
cd inferq

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
| MinIO | 9001 (console) | S3-compatible message storage |

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

1. Dashboard → **Providers** → Ollama → add backend URL (e.g. `http://host.docker.internal:11434`)
2. The health-checker (30 s interval) will detect the backend and start routing
3. Pull a model via the sync button and it appears in capacity analytics

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
JWT_SECRET=<openssl rand -hex 32>   # required — JWT signing key
OLLAMA_URL=http://host.docker.internal:11434  # default (macOS)
```

---

## API Usage

```bash
# Create an API key (dashboard → Keys → Create Key)
# Then use it with the OpenAI SDK or curl:

curl http://localhost:3001/v1/chat/completions \
  -H "Authorization: Bearer vnx_..." \
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
client = OpenAI(base_url="http://localhost:3001/v1", api_key="vnx_...")
response = client.chat.completions.create(model="llama3.2", messages=[...])
```

Interactive API docs available at `http://localhost:3002/api-docs`.

---

## Architecture

```
Client → POST /v1/chat/completions
          ↓ API key auth + rate limit
          ↓ RPUSH  veronex:queue:jobs:paid | :jobs | :jobs:test
          ↓ BLPOP  queue_dispatcher_loop
          ↓ VRAM check + thermal check + Semaphore.try_acquire()
          ↓ OllamaAdapter.stream_tokens()  (or GeminiAdapter)
          ↓ SSE stream → client
          ↓ HttpObservabilityAdapter → veronex-analytics → OTel → ClickHouse
```

Two Rust crates:
- **`veronex`** — API server (`crates/inferq/`)
- **`veronex-analytics`** — OTel ingest + analytics read API (`crates/veronex-analytics/`, port 3003)

Full architecture details: [`.ai/architecture.md`](.ai/architecture.md)
Domain CDD docs: [`docs/llm/`](docs/llm/)
Frontend CDD docs: [`docs/llm/frontend/`](docs/llm/frontend/)

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
