# Tasks: 2026-Q1

> L3: LLM autonomous | Based on scopes/2026-Q1.md

## Active

- [ ] 01: Project structure + pyproject.toml

## Pending

- [ ] 02: Domain model (InferenceJob, Model, StreamToken, enums)
- [ ] 03: ARQ worker + Valkey queue adapter
- [ ] 04: Ollama adapter (IGpuPort implementation)
- [ ] 05: Model manager (greedy allocation + LRU eviction)
- [ ] 06: SSE streaming endpoint + disconnect handling
- [ ] 07: PostgreSQL job repository (SQLAlchemy 2.0 async + Alembic)
- [ ] 08: Observability adapters (OTel / ClickHouse / stdout)
- [ ] 09: docker-compose (base + monitoring/analytics profiles)
- [ ] 10: API Key & Usage Tracking (key gen, auth middleware, rate limit, usage query)
- [ ] 11: Web Dashboard (Next.js 15 + shadcn/ui — overview, usage, perf, backend mgmt, keys)

## Completed

(none yet)

## Blocked

(none)

## Dependencies

```
01 → 02 → 03, 04
          03, 04 → 05
          05 → 06, 07
          06, 07 → 08
          08 → 09, 10, 11
          06 → 10  (disconnect → cancelled finish_reason)
          10 → 11  (API key mgmt UI)
```
