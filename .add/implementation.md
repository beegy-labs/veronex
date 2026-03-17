# Implementation

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

SDD task moves to active status.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Task spec | `.specs/veronex/<task>.md` |
| Domain docs | `docs/llm/` (relevant domain) |
| Patterns | `docs/llm/policies/patterns.md` |
| Architecture | `docs/llm/policies/architecture.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Read SDD spec, confirm requirements |
| 2 | Read CDD constraints for target domain |
| 3 | Plan approach (mental model, no doc) |
| 4 | Implement following hexagonal architecture |
| 5 | Write tests (unit + integration) |
| 6 | Run full test suite |
| 7 | Update CDD docs via `.add/cdd-feedback.md` |

## Rules

| Rule | Detail |
| ---- | ------ |
| Spec-first | Validate SDD spec before coding |
| Hexagonal | domain -> application -> infrastructure |
| CDD compliance | Check constraints before and after |
| CDD feedback | Always update docs after completion |
