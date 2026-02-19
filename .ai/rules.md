# Core Development Rules

> Essential rules for AI assistants | **Last Updated**: 2026-02-19

## Language Policy

**ALL code, documentation, and commits MUST be in English.**

## Documentation Policy (4-Tier)

| Tier | Path        | LLM Editable | Purpose                |
| ---- | ----------- | ------------ | ---------------------- |
| 1    | `.ai/`      | **YES**      | Pointer (≤50 lines)    |
| 2    | `docs/llm/` | **YES**      | SSOT (token-optimized) |
| 3    | `docs/en/`  | **NO**       | Generated              |
| 4    | `docs/kr/`  | **NO**       | Translated             |

## Architecture: Hexagonal

```
Inbound Adapters → [Ports] → Application Core → [Ports] → Outbound Adapters
  (HTTP, SSE)                  (Use Cases)                  (GPU, Redis, DB)
```

## NEVER / ALWAYS

| NEVER                         | Alternative                     |
| ----------------------------- | ------------------------------- |
| Business logic in adapters    | Use application layer use cases |
| Direct GPU call outside ports | Use InferencePort               |
| Hardcode secrets              | Use env/ConfigService           |
| Block GPU without queue       | Always enqueue first            |

| ALWAYS                        | Details                         |
| ----------------------------- | ------------------------------- |
| Enqueue before GPU work       | Serial processing guaranteed    |
| Stream via SSE                | Real-time token delivery        |
| Define ports before adapters  | Dependency rule respected       |

**SSOT**: `docs/llm/rules.md`
