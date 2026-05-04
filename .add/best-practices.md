# Best Practices

> ADD Execution | **Last Updated**: 2026-04-22
> Shared constants (Scale targets, Verification commands, CDD Sync Routing) → [`.add/README.md`](README.md)

## Role

Two workflows:

1. **Update workflow** — when and how to update `docs/llm/policies/` docs
2. **Refactor workflow** — align existing code to current best practices

---

## Part 1 — Update

### Triggers

| Trigger | Target doc |
|---------|-----------|
| Same issue repeated 2+ times in code review | `patterns.md` or `patterns-frontend.md` |
| New architectural decision | `architecture.md` |
| Security/performance incident post-mortem | `patterns.md` or `auth/security.md` |
| New tech stack adoption | Relevant domain doc |
| Dependency major version bump | `patterns-frontend.md` + `testing-strategy.md` |
| Quarterly audit | All `docs/llm/policies/` |

### Where to write what

| Doc | Content |
|-----|---------|
| `docs/llm/policies/patterns.md` | Rust code patterns + quarterly audit grep commands |
| `docs/llm/policies/patterns-frontend.md` | TypeScript/React/Next.js patterns — TanStack Query, tokens, i18n, perf, a11y, lucide-react, React APIs |
| `docs/llm/policies/architecture.md` | Layer structure, hexagonal boundaries, crate dependency rules |
| `docs/llm/policies/testing-strategy.md` | Test writing rules — Testing Trophy 5-Layer (frontend + Rust), behavior-driven rules, proptest/insta/wiremock/testcontainers, cargo-mutants cadence, Axum `oneshot` handler tests |
| `docs/llm/frontend/execution-contracts.md` | Feature folder structure, state classification, naming conventions, common module import contract, feature boundary rules |
| `docs/llm/frontend/design-system.md` | Brand, tokens, theme, nav, DataTable, platform APIs (Next.js/React version notes) |
| `docs/llm/frontend/design-system-components.md` | Auth guard, login, shared component inventory, status colors |
| `docs/llm/frontend/design-system-components-patterns.md` | Component patterns — ConfirmDialog, 2-Step Verify, NavigationProgressProvider, useApiMutation |
| `docs/llm/frontend/design-system-i18n.md` | i18n config, timezone, date formatters |
| `docs/llm/frontend/charts.md` | Recharts patterns, formatters, SSOT for chart theme |
| `docs/llm/frontend/pages/{page}.md` | Per-page architecture, components, types, i18n keys, known violations |
| `docs/llm/flows/{subsystem}.md` | Control flow and algorithm for one subsystem — read before implementing, update when logic changes |

Rule: if a pattern applies across pages → `policies/patterns-frontend.md`. If it's page-specific → `pages/{page}.md`. Never duplicate the same rule in both.

Rule: domain structure, state classification, common module contracts → `execution-contracts.md`. Never put these in `patterns-frontend.md`.

Rule: flows docs are algorithm contracts. When logic in a subsystem changes, update the corresponding `flows/` doc in the same commit.

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
| 2 | Find violations — `audit-backend.md` (Rust), `audit-frontend.md` (FE), `audit-security.md` (OWASP) |
| 3 | Prioritize — P1 (security/correctness) → P2 (arch/perf) → P3 (quality) |
| 4 | Fix in rounds — one rule, one file group at a time |
| 5 | Verify each round — `cargo check --workspace` (Rust) or `npx tsc --noEmit` (Frontend) |
| 6 | Full test — `cargo nextest run --workspace` (Rust) or Playwright (Frontend) |
| 7 | CDD sync — update policies doc if new pattern discovered |

### Rules

| Rule | Detail |
|------|--------|
| Preserve behavior | No logic changes during refactor |
| Round-based | Verify after each round |
| Scope limit | No refactoring outside requested modules |
| Tests must pass | Green state after all rounds |

---

## Part 3 — Frontend Audit

Grep blocks (P1 Security & Correctness, P2 Architecture & Performance, P3 Quality) → [`audit-frontend.md`](audit-frontend.md).

---


## Part 4 — LLM Gateway Security Audit

P0/P1/P2 greps for GPU monopoly + header hardening (OWASP API + LLM 2025) → [`audit-security.md`](audit-security.md).

---


## Part 5 — Backend (Rust) Audit

P1 Architecture, P2 Performance & Scale, P3 Observability, P4 Testing grep blocks → [`audit-backend.md`](audit-backend.md).

