# Best Practices

> ADD Execution | **Last Updated**: 2026-03-24

## Role

Two workflows:

1. **Update workflow** — when and how to update `docs/llm/policies/` docs
2. **Refactor workflow** — align existing code to current best practices

---

## Part 1 — Update

### Triggers

| Trigger | Target doc |
|---------|-----------|
| Same issue repeated 2+ times in code review | `patterns.md` |
| New architectural decision | `architecture.md` |
| Security/performance incident post-mortem | `patterns.md` or `auth/security.md` |
| New tech stack adoption | Relevant domain doc |
| Quarterly audit | All `docs/llm/policies/` |

### Where to write what

| Doc | Content |
|-----|---------|
| `docs/llm/policies/patterns.md` | Rust code patterns + quarterly audit grep commands |
| `docs/llm/policies/patterns-frontend.md` | TypeScript/Next.js patterns + review priority |
| `docs/llm/policies/architecture.md` | Layer structure, hexagonal boundaries, crate dependency rules |
| `docs/llm/policies/testing-strategy.md` | Test writing rules |

### Steps

| Step | Action |
|------|--------|
| 1 | Identify trigger — define what pattern to add/update/remove |
| 2 | Read the target doc section |
| 3 | Write concisely — include WHY + applicability conditions, not just WHAT |
| 4 | Grep for existing violations of the new rule |
| 5 | Violations found → enter Part 2 refactor workflow |
| 6 | Update `Last Updated` date |

---

## Part 2 — Refactor

### Trigger

- Violations found in quarterly audit (`patterns.md` Quarterly Audit Commands section)
- Repeated violations found in code review
- New best practice rule established, existing code needs alignment

### Steps

| Step | Action |
|------|--------|
| 1 | Define scope — which rule, which module vs whole codebase |
| 2 | Find violations — run audit commands from `patterns.md` |
| 3 | Prioritize — P1 (security/correctness) → P2 (arch/perf) → P3 (quality) |
| 4 | Fix in rounds — one rule, one file group at a time |
| 5 | Verify each round — `cargo check --workspace` |
| 6 | Full test — `cargo nextest run --workspace` |
| 7 | CDD sync — update policies doc if new pattern discovered |

### Rules

| Rule | Detail |
|------|--------|
| Preserve behavior | No logic changes during refactor |
| Round-based | `cargo check` after each round |
| Scope limit | No refactoring outside requested modules |
| Tests must pass | Green state after all rounds |
