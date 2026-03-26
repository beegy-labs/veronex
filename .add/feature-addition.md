# Feature Addition

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

User requests new feature or SDD spec moves to active.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Feature spec | `.specs/veronex/<feature>.md` |
| Domain docs | `docs/llm/` (relevant domain) |
| Patterns | `docs/llm/policies/patterns.md` |
| Architecture | `docs/llm/policies/architecture.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Verify SDD spec exists; if not, create before coding |
| 2 | Read CDD constraints for target domain |
| 3 | Implement — hexagonal for `veronex`; flat module for `veronex-mcp` (see `infra/crate-structure.md`) |
| 4 | Write tests (unit + integration) |
| 5 | CDD feedback — run `.add/cdd-feedback.md` |

## Rules

| Rule | Detail |
| ---- | ------ |
| Spec-first | No code without SDD spec |
| veronex: hexagonal | domain -> application (ports) -> infrastructure (adapters) |
| veronex-mcp: flat module | Tool trait + tools/{name}.rs — no hexagonal layers |
| Test before commit | All new code must have tests |
| CDD feedback | Always run after completion |
