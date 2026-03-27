# Refactor

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

User requests refactoring, or code review identifies structural issues.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Review output | `.add/code-review.md` results |
| Patterns | `docs/llm/policies/patterns.md` |
| Testing | `docs/llm/policies/testing-strategy.md` |
| Architecture | `docs/llm/policies/architecture.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Capture current behavior with tests |
| 2 | Refactor in small rounds (fix -> verify -> repeat) |
| 3 | Run full test suite after each round |
| 4 | CDD feedback — run `.add/cdd-feedback.md` if new patterns confirmed |

## Rules

| Rule | Detail |
| ---- | ------ |
| Behavior-preserving | No logic changes during refactor |
| Round-based | Small steps, verify after each |
| Tests must pass | Every round ends with green tests |
| CDD feedback | Only if new stable pattern confirmed — not for every refactor |
