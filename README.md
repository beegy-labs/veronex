# Veronex

**Autonomous scheduler and gateway for N Ollama servers** — VRAM-aware routing, adaptive concurrency, thermal protection, OpenAI-compatible API.

Veronex is not a reverse proxy. It treats all your Ollama instances as a single compute pool and learns the optimal request concurrency per model through live inference data.

- Routes requests to the provider with the most available VRAM, with model-stickiness to avoid reloading
- Learns `max_concurrent` per model via AIMD (TPS + p95 latency), then refines with LLM batch analysis
- Detects GPU/CPU thermal state automatically and throttles before hardware is stressed
- Recovers from provider crashes via circuit breaker + Valkey queue reaper
- OpenAI, Ollama native, and Gemini API compatible — drop-in for existing clients

Primary optimization target: **AMD Ryzen AI 395+** (APU unified memory).

---

## Quick Start

```bash
git clone <repo> && cd veronex
cp .env.example .env          # set JWT_SECRET (required)
docker compose up -d
open http://localhost:3002     # setup wizard → create admin → add provider
```

> **macOS**: `OLLAMA_URL=http://host.docker.internal:11434` works out of the box.
> **Linux**: set `OLLAMA_URL=http://172.17.0.1:11434` in `.env`.

API key: Dashboard → Keys → Create Key.

---

## API Usage

Drop-in OpenAI compatible:

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

Interactive docs: `http://localhost:3001/swagger-ui`

---

## How It Works

### Request Flow

```mermaid
flowchart TD
    C([Client]) --> M[API Key Auth + Rate Limit]
    M -->|rejected| E([401 / 429])
    M -->|ok| Q[Valkey Priority Queue\npaid › standard › test]

    Q --> D[Queue Dispatcher]
    D --> F[Provider Filter\n① active + type\n② model available\n③ admin-enabled]
    F -->|none| FAIL([job → failed])
    F -->|ok| S[Score by VRAM headroom\n+ model stickiness]

    S --> CB{Circuit Breaker}
    CB -->|open| S
    CB -->|ok| TH{Thermal}
    TH -->|hard| S
    TH -->|ok| VR{VRAM reserve}
    VR -->|full| RQ[re-enqueue]
    VR -->|ok| RUN[Job Runner]

    RUN --> OL[Ollama]
    RUN --> GM[Gemini]
    OL & GM --> SSE[SSE stream → client]
    SSE --> DROP[VramPermit drop\nKV cache released]
    DROP --> OBS[(Postgres + ClickHouse)]
```

### Adaptive Concurrency (per provider, per model)

```mermaid
flowchart LR
    subgraph SYNC["Every 30s per provider"]
        PS[GET /api/ps\nloaded models] --> VRAM{DRM VRAM\n< model weight?}
        VRAM -->|APU| EST[estimated = observed × 1.15]
        VRAM -->|GPU| DRM[total = DRM reported]
        EST & DRM --> POOL[VramPool update]
        POOL --> KV[KV cache estimate\nfrom arch profile]
        KV --> AI{samples ≥ 3?}
        AI -->|cold| INIT[weight table\n<5GB→8 · 5-20GB→4\n20-50GB→2 · >50GB→1]
        AI -->|warm| AIMD[AIMD\ntps<0.7× → ×0.75\ntps≥0.9× → +1]
        INIT & AIMD --> LLM[LLM Batch\nqwen2.5:3b ±2 clamp]
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
