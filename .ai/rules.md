# Core Development Rules

> CDD Tier 1 — Essential rules for AI assistants | **Last Updated**: 2026-03-03

## Language Policy

**ALL code, documentation, and commits MUST be in English.**

## Documentation Policy (4-Tier)

| Tier | Path        | LLM Editable | Purpose                |
| ---- | ----------- | ------------ | ---------------------- |
| 1    | `.ai/`      | **YES**      | Pointer (≤50 lines)    |
| 2    | `docs/llm/` | **YES**      | SSOT (topic-based)     |
| 3    | `docs/en/`  | **NO**       | Generated              |
| 4    | `docs/kr/`  | **NO**       | Translated             |

**Never edit `docs/en/` or `docs/kr/` directly.**

`docs/llm/` structure: `policies/` (architecture, git-flow) + topic docs (backends, hardware, jobs, infrastructure, web)

## Agentic Dev Protocol

`vendor/agentic-dev-protocol/` (git submodule)
→ https://github.com/beegy-labs/agentic-dev-protocol

Upstream SSOT for dev process and workflow policies. Veronex-specific rules are managed in `.ai/` and `docs/llm/policies/`.

## Architecture: Hexagonal

```
Inbound Adapters → [Ports] → Application Core → [Ports] → Outbound Adapters
  (HTTP, SSE)                  (Use Cases)                  (Valkey, Postgres, OTel)
```

## NEVER / ALWAYS

| NEVER                           | Alternative                       |
| ------------------------------- | --------------------------------- |
| Business logic in adapters      | Use application layer use cases   |
| Direct GPU call outside ports   | Use InferenceBackendPort          |
| Hardcode secrets                | Use environment variables         |
| Dispatch without queue          | Always RPUSH to `veronex:queue:jobs` |
| Edit `docs/en/` or `docs/kr/`  | Edit `.ai/` or `docs/llm/` only   |
| Hardcode CSS colors in components | Reference `--theme-*` tokens    |
| Use `Uuid::new_v4()` or `gen_random_uuid()` for PKs | Use `Uuid::now_v7()` (app) / `uuidv7()` (PG18) |

| ALWAYS                          | Details                           |
| ------------------------------- | --------------------------------- |
| Enqueue before GPU work         | `RPUSH veronex:queue:jobs`        |
| Stream via SSE                  | Real-time token delivery          |
| Define ports before adapters    | Dependency rule respected         |
| Use `--theme-*` tokens in CSS   | `tokens.css` is the design SSOT   |
| Check docs/llm/ before coding   | CDD-first: update docs then code  |
| Use `onSettled` for TQ invalidation | Runs on error too (not `onSuccess`) |
| New background loop → accept `CancellationToken` | Enables graceful shutdown via `JoinSet` |
| Gate Gemini (and any lab feature) UI via `useLabSettings()` | Never read lab settings in local component state — use the context SSOT |
| Always display `estimated_cost_usd` when showing job or usage data | Never hide the field; Ollama backends always show $0.00 — see `docs/llm/backend/model-pricing.md` |
| Never substitute a missing pricing row with silence | Show `$0.00` for Ollama, `—` / null for other providers without a pricing entry |
