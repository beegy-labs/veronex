# Core Development Rules

> CDD Layer 1 — Essential rules for AI assistants | **Last Updated**: 2026-03-15

## Language Policy

**ALL code, documentation, and commits MUST be in English.**

## Documentation Policy (3-Layer)

| Layer | Path | Editable | Purpose |
| ----- | ---- | -------- | ------- |
| 1 | `.ai/` | **YES** | Pointer (≤50 lines) |
| 2 | `docs/llm/` | **YES** | SSOT (domain-based, machine-optimized) |
| 3 | `docs/en/`, `docs/kr/` | **NO** | Human understanding (generated/translated) |

**Never edit Layer 3 directly. Edit Layer 1 or Layer 2 only.**

Layer 3 strategy: Policy docs use vendor symlinks. Project-specific Layer 3 generation is deferred until team onboarding requires it.

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
| Dispatch without queue | ZADD to ZSET priority queue (`veronex:queue:zset`, tier-scored) |
| Edit `docs/en/` or `docs/kr/` | Edit `.ai/` or `docs/llm/` only |
| Hardcode CSS colors | Reference `--theme-*` tokens |
| `Uuid::new_v4()` for PKs | Use `Uuid::now_v7()` (app) / `uuidv7()` (PG18). Exception: `instance_id` uses v4 (random, non-PK) |

| ALWAYS | Details |
| ------ | ------- |
| Enqueue before GPU work | ZSET queue: `veronex:queue:zset` (score = now_ms - tier_bonus) |
| Stream via SSE | Real-time token delivery |
| Define ports before adapters | Dependency rule respected |
| Use `--theme-*` tokens in CSS | `tokens.css` is the design SSOT |
| Check docs/llm/ before coding | CDD-first: update docs then code |
| Use `onSettled` for TQ invalidation | Runs on error too |
| Gate lab features via `useLabSettings()` | Context SSOT, not local state |
