# Core Development Rules

> CDD Tier 1 — Essential rules for AI assistants | **Last Updated**: 2026-03-07

## Language Policy

**ALL code, documentation, and commits MUST be in English.**

## Documentation Policy (4-Tier)

| Tier | Path | LLM Editable | Purpose |
| ---- | ---- | ------------ | ------- |
| 1 | `.ai/` | **YES** | Pointer (≤50 lines) |
| 2 | `docs/llm/` | **YES** | SSOT (domain-based) |
| 3 | `docs/en/` | **NO** | Generated |
| 4 | `docs/kr/` | **NO** | Translated |

**Never edit `docs/en/` or `docs/kr/` directly.**

## Architecture: Hexagonal

```
Inbound Adapters → [Ports] → Application Core → [Ports] → Outbound Adapters
  (HTTP, SSE)                  (Use Cases)                  (Valkey, Postgres, OTel)
```

## NEVER / ALWAYS

| NEVER | Alternative |
| ----- | ----------- |
| Business logic in adapters | Use application layer use cases |
| Hardcode secrets | Use environment variables |
| Dispatch without queue | RPUSH to priority queues (paid > api > test) |
| Edit `docs/en/` or `docs/kr/` | Edit `.ai/` or `docs/llm/` only |
| Hardcode CSS colors | Reference `--theme-*` tokens |
| `Uuid::new_v4()` for PKs | Use `Uuid::now_v7()` (app) / `uuidv7()` (PG18). Exception: `instance_id` uses v4 (random, non-PK) |

| ALWAYS | Details |
| ------ | ------- |
| Enqueue before GPU work | 3-queue: `veronex:queue:jobs:paid`, `veronex:queue:jobs`, `veronex:queue:jobs:test` |
| Stream via SSE | Real-time token delivery |
| Define ports before adapters | Dependency rule respected |
| Use `--theme-*` tokens in CSS | `tokens.css` is the design SSOT |
| Check docs/llm/ before coding | CDD-first: update docs then code |
| Use `onSettled` for TQ invalidation | Runs on error too |
| Gate lab features via `useLabSettings()` | Context SSOT, not local state |
