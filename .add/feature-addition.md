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
| 3 | Implement following hexagonal architecture |
| 4 | Write tests (unit + integration) |
| 5 | Update CDD docs with new patterns/constraints |

## Rules

| Rule | Detail |
| ---- | ------ |
| Spec-first | No code without SDD spec |
| Hexagonal | domain -> application (ports) -> infrastructure (adapters) |
| Test before commit | All new code must have tests |
| CDD feedback | Update docs after completion |
