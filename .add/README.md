# ADD — Workflow Index

> ADD (Agent Decision Document) | **Last Updated**: 2026-04-22
> Execution workflows for Claude Code. CDD = `docs/llm/` | SDD = `.specs/`

## Trigger → Workflow Map

| Trigger | Workflow |
|---------|----------|
| Code review, optimization, review of files | [`code-review.md`](code-review.md) |
| Frontend-only review | [`frontend-review.md`](frontend-review.md) |
| Backend-only review (Rust) | [`backend-review.md`](backend-review.md) |
| Writing/updating a backend test (any layer) | [`backend-test.md`](backend-test.md) |
| New backend handler / domain / adapter | [`backend-feature.md`](backend-feature.md) |
| New feature / SDD spec active | [`feature-addition.md`](feature-addition.md) |
| Refactor requested / structural issue | [`refactor.md`](refactor.md) |
| Bug report / test failure | [`bug-fix.md`](bug-fix.md) |
| DB migration / schema change | [`migration.md`](migration.md) |
| Dependency version bump / CVE | [`dependency-upgrade.md`](dependency-upgrade.md) |
| Best practices update / refactor alignment | [`best-practices.md`](best-practices.md) |
| Doc alignment / CDD sync | [`doc-sync.md`](doc-sync.md) |
| CDD feedback after task completion | → see [`best-practices.md`](best-practices.md) Part 1 |
| Commit message CI failure | [`commit-fix.md`](commit-fix.md) |
| Security review / OWASP audit | [`best-practices.md`](best-practices.md) Part 4 |
| Backend / infrastructure E2E suite execution | [`e2e-test.md`](e2e-test.md) |
| Uncertainty / ambiguous requirements | [`escalation.md`](escalation.md) |

## Shared Constants (referenced by all workflows)

### Scale Targets

All code — Rust and frontend — is written and reviewed against:

| Axis | Target |
|------|--------|
| Providers (Ollama servers) | **10,000** |
| MCP servers | **1,000+** |
| Concurrent requests (TPS) | **1,000,000** |

No O(N) DB scans, sequential awaits, or unbounded memory growth at these scales. Flag violations P1+.

### Verification Commands

| Domain | Command | When |
|--------|---------|------|
| Rust compile | `cargo check --workspace` | After every Rust change |
| Rust lint | `cargo clippy --all-targets` | Before commit |
| Rust tests | `cargo nextest run --workspace` | Before commit |
| Rust deps audit | `cargo deny check` | Before PR |
| Rust mutation (PR diff) | `cargo mutants --in-diff origin/develop --timeout 30` | In CI per PR |
| Frontend compile | `npx tsc --noEmit` | After every TSX/TS change |
| Frontend unit | `npx vitest run` | Before commit |
| Frontend E2E | `npx playwright test` | Before PR |

### CDD Sync Routing

When a new pattern is established, update the doc that owns it. See [`best-practices.md`](best-practices.md) Part 1 — "Where to write what" for the full routing table.

Quick reference:

| What changed | Target doc |
|---|---|
| Rust code pattern | `docs/llm/policies/patterns.md` |
| Frontend pattern (query, token, i18n, perf) | `docs/llm/policies/patterns-frontend.md` |
| Architecture boundary | `docs/llm/policies/architecture.md` |
| Test pattern | `docs/llm/policies/testing-strategy.md` |
| Page-specific component/type | `docs/llm/frontend/pages/{page}.md` |
| Design token / nav / DataTable | `docs/llm/frontend/design-system.md` |
| Component pattern (ConfirmDialog, 2-Step Verify) | `docs/llm/frontend/design-system-components-patterns.md` |
| DB schema | `docs/llm/` affected domain |
| MCP integration / tool retrieval | `docs/llm/inference/mcp-schema.md` |
| Crate structure (embed, agent, mcp) | `docs/llm/infra/crate-structure.md` |
| **Control flow / algorithm change** | **`docs/llm/flows/{subsystem}.md`** |

## File Ownership

| File | SSOT for |
|------|----------|
| `README.md` (this file) | Scale targets, verification commands, CDD sync routing — shared by all workflows |
| `best-practices.md` | "Where to write what" routing, Part 1 update workflow, Part 2 refactor workflow |
| `audit-frontend.md` | P1/P2/P3 frontend grep audit blocks |
| `audit-backend.md` | P1/P2/P3/P4 Rust (backend) grep audit blocks |
| `audit-security.md` | P0/P1/P2 LLM gateway security greps (OWASP API + LLM 2025) |
| `skills.md` | Tech stack inventory, version changelog |
| `escalation.md` | Decision table — escalate vs. proceed |
| `code-review.md` | Full review workflow (Rust + frontend), P1/P2/P3 severity definitions |
| `frontend-review.md` | Frontend-only review scope, parallel agent structure (Reuse/Quality/Efficiency) |
| `backend-review.md` | Backend-only (Rust) review scope, parallel agent structure (Reuse/Quality/Efficiency), architecture non-goals |
| `backend-feature.md` | New Rust handler / domain / adapter workflow |
| `backend-test.md` | Rust Testing Trophy (5-Layer) — layer selection + behavior-driven rules |
| `feature-addition.md` | New feature workflow (also covers `implementation.md` triggers) |
| `implementation.md` | Redirect → `feature-addition.md` |
| `refactor.md` | Structural refactor workflow |
| `bug-fix.md` | Bug diagnosis + fix workflow |
| `migration.md` | DB schema migration workflow |
| `dependency-upgrade.md` | Rust crate + npm upgrade workflow, status tables, migration notes |
| `doc-sync.md` | CDD doc alignment workflow |
| `cdd-feedback.md` | Post-task CDD feedback classification (operational vs. constitutional) |
| `commit-fix.md` | Commit message CI failure recovery, rebase scripts |
| `e2e-test.md` | E2E test suite execution workflow, script ordering, pass/fail criteria |
