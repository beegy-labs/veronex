# Doc Sync

> ADD Execution | Last Updated: 2026-03-14

## Trigger

User requests doc cleanup, CDD sync, or doc alignment check.

## Principle

| Rule | Detail |
| ---- | ------ |
| Code is SSOT | Docs describe code, not the other way around |
| No duplication | One fact in one place, reference elsewhere |
| Token optimization | All Tier 1/2 docs follow [token-optimization.md](../docs/llm/policies/token-optimization.md) |

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| CDD policy | `docs/llm/policies/cdd.md` |
| Token rules | `docs/llm/policies/token-optimization.md` |
| Terminology | `docs/llm/policies/terminology.md` |
| Doc index | `docs/llm/README.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Read current code to establish ground truth |
| 2 | Compare CDD docs against code |
| 3 | Fix docs that diverge from code |
| 4 | Apply token optimization (no emoji, no prose, tables over bullets) |
| 5 | Remove duplicate content across docs |
| 6 | Update `docs/llm/README.md` index if docs added/removed |

## Rules

| Rule | Detail |
| ---- | ------ |
| Code wins | If doc contradicts code, fix the doc |
| Scope: Tier 1 | `.ai/` — pointers only, max 500 tokens (~50 lines) |
| Scope: Tier 2 | `docs/llm/` — SSOT, max 2,000 tokens (pages/ max 1,500) |
| No orphan docs | Every doc must be indexed in `docs/llm/README.md` |
| Scope | Target docs only, no code changes |
