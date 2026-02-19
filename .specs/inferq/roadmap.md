# inferq Roadmap

> L1: Master direction | Load on planning only

## Vision

Open-source, queue-based LLM inference server. Single GPU (APU), multiple models, real-time SSE streaming. Pluggable backends (Ollama → llama.cpp → vLLM).

## Phases

| Phase | Priority | Scope | Status |
| ----- | -------- | ----- | ------ |
| Q1 | P0 | MVP: Ollama + Queue + SSE + Model Manager | → scopes/2026-Q1.md |
| Q2 | P1 | Observability: OTel + ClickHouse analytics | Pending |
| Q3 | P1 | llama.cpp adapter + backend abstraction | Pending |
| Q4 | P2 | Priority queue + multi-tenant + auth | Pending |

## Q1 MVP Scope

Core system that works end-to-end:

1. Hexagonal project structure (FastAPI + ARQ + Valkey)
2. Domain model (InferenceJob, Model, StreamToken)
3. Ollama adapter (`IGpuPort`)
4. Model manager (greedy allocation + LRU eviction)
5. Queue worker (ARQ, max_jobs=1)
6. SSE streaming endpoint (sse-starlette)
7. PostgreSQL job state (SQLAlchemy 2.0 async + Alembic)
8. Observability skeleton (OTel + ClickHouse/stdout adapters)
9. docker-compose (inferq + valkey + postgres + otel-collector)

## Q2 Observability Scope

1. ClickHouse analytics tables (`inference_logs`, `model_metrics`)
2. Prometheus `/metrics` endpoint (custom metrics)
3. Grafana dashboard template
4. OTel traces for end-to-end request tracking

## Dependencies

- Q1 → Q2 (observability needs working inference)
- Q1 → Q3 (llama.cpp needs same port interface)

## References

- Best Practices: `references/best-practices.md`
- Architecture: `docs/llm/policies/architecture.md` (CDD)
