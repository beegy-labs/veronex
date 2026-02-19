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
          08 → 09
```
