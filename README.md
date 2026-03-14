# Veronex

**Autonomous scheduler and gateway for N Ollama servers** — VRAM-aware routing, adaptive concurrency, thermal protection, OpenAI-compatible API.

Veronex is not a reverse proxy. It treats all your Ollama instances as a single compute pool and learns the optimal concurrency per model through live inference data.

- **Smart routing** — dispatches to the provider with the most VRAM headroom; keeps models resident to avoid reloading
- **Adaptive concurrency** — learns `max_concurrent` per model via AIMD (TPS + p95), then refines via LLM batch analysis
- **Thermal protection** — detects GPU/CPU thermal state per provider; throttles automatically before hardware is stressed
- **Self-healing** — circuit breaker + queue reaper recover from provider crashes without losing requests
- **API compatible** — OpenAI, Ollama native, and Gemini — drop-in for existing clients and SDKs

---

## Quick Start

```bash
git clone <repo> && cd veronex
cp .env.example .env          # set JWT_SECRET (required)
docker compose up -d
open http://localhost:3002     # setup wizard → create admin → add provider → get API key
```

> **macOS**: `OLLAMA_URL=http://host.docker.internal:11434` works out of the box.
> **Linux**: set `OLLAMA_URL=http://172.17.0.1:11434` in `.env`.

Then call it like any OpenAI-compatible endpoint:

```bash
curl http://localhost:3001/v1/chat/completions \
  -H "Authorization: Bearer iq_..." \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2", "messages": [{"role": "user", "content": "Hello"}], "stream": true}'
```

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:3001/v1", api_key="iq_...")
client.chat.completions.create(model="llama3.2", messages=[...])
```

Interactive API docs: `http://localhost:3001/swagger-ui`

---

## How It Works

### Request Flow

```mermaid
flowchart TD
    C([Client]) --> M[API Key Auth + Rate Limit]
    M -->|rejected| E([401 / 429])
    M -->|ok| Q[Valkey Priority Queue\npaid › standard › test]

    Q --> D[Queue Dispatcher]
    D --> F[Provider Filter\n① active + type match\n② model available\n③ admin-enabled]
    F -->|no match| FAIL([job → failed])
    F -->|candidates| S[Score & rank\nVRAM headroom + model stickiness]

    S --> GATE["For each candidate in order"]
    GATE -->|circuit open| GATE
    GATE -->|thermal hard| GATE
    GATE -->|VRAM full| RQ[re-enqueue to front]
    GATE -->|pass| RUN[Job Runner]

    RUN --> OL[Ollama]
    RUN --> GM[Gemini]
    OL & GM --> SSE[SSE stream → client]
    SSE --> DROP[VramPermit drop — KV cache released]
    DROP --> OBS[(Postgres + ClickHouse)]
```

### Adaptive Concurrency (per provider × model, every 30s)

```mermaid
flowchart LR
    subgraph SYNC["Sync Loop"]
        PS[GET /api/ps] --> VRAM{loaded weight\n> DRM VRAM?}
        VRAM -->|APU unified memory| EST[estimated = observed × 1.15]
        VRAM -->|discrete GPU| DRM[total = DRM reported]
        EST & DRM --> POOL[VramPool update]
        POOL --> KV[KV cache estimate\nfrom model arch]
        KV --> PHASE{samples ≥ 3?}
        PHASE -->|cold start| INIT[weight table\n<5GB→8 · 5-20GB→4\n20-50GB→2 · >50GB→1]
        PHASE -->|learning| AIMD[AIMD\ntps<0.7× → ×0.75\ntps≥0.9× → +1\np95 spike → decrease]
        INIT & AIMD --> LLM[LLM Batch\nqwen2.5:3b · ±2 clamp]
    end
```

---

## Tech Stack

| | |
|-|-|
| **API server** | Rust · Axum 0.8 · tokio · SSE |
| **Scheduler** | Valkey (Lua priority queue) · PostgreSQL 18 |
| **Analytics** | ClickHouse · OTel Collector · Redpanda |
| **Dashboard** | Next.js 16 · Tailwind v4 · shadcn/ui |
| **Deploy** | Docker Compose · Kubernetes (Helm) |

---

## License

MIT
