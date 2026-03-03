# Core Development Rules

> CDD Tier 1 — Essential rules for AI assistants | **Last Updated**: 2026-03-03

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

`docs/llm/` structure: `policies/` + `auth/` + `inference/` + `providers/` + `infra/` + `frontend/` + `research/`

## Agentic Dev Protocol

`vendor/agentic-dev-protocol/` (git submodule) — upstream SSOT for CDD/SDD/ADD policies.

## Architecture: Hexagonal

```
Inbound Adapters → [Ports] → Application Core → [Ports] → Outbound Adapters
  (HTTP, SSE)                  (Use Cases)                  (Valkey, Postgres, OTel)
```

## NEVER / ALWAYS

| NEVER | Alternative |
| ----- | ----------- |
| Business logic in adapters | Use application layer use cases |
| Direct GPU call outside ports | Use InferenceBackendPort |
| Hardcode secrets | Use environment variables |
| Dispatch without queue | Always RPUSH to `veronex:queue:jobs` |
| Edit `docs/en/` or `docs/kr/` | Edit `.ai/` or `docs/llm/` only |
| Hardcode CSS colors | Reference `--theme-*` tokens |
| `Uuid::new_v4()` for PKs | Use `Uuid::now_v7()` (app) / `uuidv7()` (PG18) |

| ALWAYS | Details |
| ------ | ------- |
| Enqueue before GPU work | `RPUSH veronex:queue:jobs` |
| Stream via SSE | Real-time token delivery |
| Define ports before adapters | Dependency rule respected |
| Use `--theme-*` tokens in CSS | `tokens.css` is the design SSOT |
| Check docs/llm/ before coding | CDD-first: update docs then code |
| Use `onSettled` for TQ invalidation | Runs on error too |
| Gate lab features via `useLabSettings()` | Context SSOT, not local state |
