# Refactor

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

User requests refactoring, or code review identifies structural issues.

## Read Before Execution

| Domain | Path | When |
|--------|------|------|
| Review output | `.add/code-review.md` results | Always |
| Code patterns (Rust) | `docs/llm/policies/patterns.md` | Rust changes |
| Code patterns (Frontend) | `docs/llm/policies/patterns-frontend.md` | Frontend changes |
| Architecture | `docs/llm/policies/architecture.md` | Structural changes |
| Testing | `docs/llm/policies/testing-strategy.md` | Test refactors |

## Execution

The refactor workflow is defined in [`best-practices.md`](best-practices.md) Part 2.

Quick summary:

| Step | Action |
|------|--------|
| 1 | Define scope — which rule, which module |
| 2 | Find violations — Part 3 greps (Frontend) or `patterns.md` audit commands (Rust) |
| 3 | Prioritize — P1 → P2 → P3 |
| 4 | Fix in rounds — one rule, one file group at a time |
| 5 | Verify each round — see `.add/README.md` Verification Commands |
| 6 | CDD sync — route per `.add/README.md` CDD Sync Routing |

## Rules

| Rule | Detail |
|------|--------|
| Behavior-preserving | No logic changes — refactor only |
| Round-based | Verify after each round |
| Scope limit | No refactoring outside requested modules |
| Tests must pass | Green state after all rounds |
