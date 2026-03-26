# Doc Sync

> ADD Execution | **Last Updated**: 2026-03-26

## Trigger

| Trigger | Source |
|---------|--------|
| User requests doc cleanup or CDD alignment check | explicit |
| Code review Step 7 — new pattern established during review | `code-review.md` |
| Same violation found 2+ times in review | `best-practices.md` Part 1 |
| Post-task CDD feedback needed | `cdd-feedback.md` |

## Principle

| Rule | Detail |
|------|--------|
| Code is SSOT | Docs describe code, not the other way around |
| No duplication | One fact in one place, reference elsewhere |
| Token optimization | All Layer 1/2 docs follow `docs/llm/policies/token-optimization.md` |

## Read Before Execution

| Domain | Path |
|--------|------|
| CDD policy | `docs/llm/policies/cdd.md` |
| Token rules | `docs/llm/policies/token-optimization.md` |
| Terminology | `docs/llm/policies/terminology.md` |
| Doc index | `docs/llm/README.md` |
| Where to write what | `.add/best-practices.md` Part 1 |

## Execution

| Step | Action |
|------|--------|
| 1 | Identify scope — user-specified domain, or `git diff` to find recently changed files |
| 2 | Read ground truth code for that domain (see Ground Truth by Domain below) |
| 3 | Read the CDD doc(s) that should reflect that code |
| 4 | List divergences — doc says X, code does Y |
| 5 | Fix docs to match code; never change code in this workflow |
| 6 | Classify each change: Operational (accumulate freely) or Constitutional (flag for human approval) |
| 7 | Apply token optimization — tables over bullets, no emoji, no prose |
| 8 | Remove duplicate content across docs |
| 9 | Update `Last Updated` date; update `docs/llm/README.md` index if docs added/removed |

## Ground Truth by Domain

| Domain | Read Code From | Compare Against |
|--------|---------------|-----------------|
| Rust handler patterns | `crates/veronex/src/infrastructure/inbound/http/*.rs` | `docs/llm/policies/patterns.md` |
| Auth / security | handler auth extractors, `auth_handlers.rs` | `docs/llm/auth/security.md` |
| Frontend query/mutation | `web/lib/queries/*.ts`, `web/app/**/*.tsx` | `docs/llm/policies/patterns-frontend.md` |
| Design system | `web/components/**/*.tsx`, `web/app/globals.css` | `docs/llm/frontend/design-system.md` |
| i18n | `web/messages/en.json`, `web/i18n.ts` | `docs/llm/frontend/design-system-i18n.md` |
| DB schema | `migrations/*.sql` | relevant domain doc in `docs/llm/` |
| Architecture | `crates/*/Cargo.toml`, crate boundaries | `docs/llm/policies/architecture.md` |
| Testing | `web/e2e/*.spec.ts`, `crates/**/tests/` | `docs/llm/policies/testing-strategy.md` |

## CDD Sync Routing

One fact → one doc. Route the updated pattern to the correct owner.

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

## Rules

| Rule | Detail |
|------|--------|
| Code wins | If doc contradicts code, fix the doc |
| No code changes | This workflow targets docs only |
| Scope: Layer 1 | `.ai/` — pointers only, max 500 tokens (~50 lines) |
| Scope: Layer 2 | `docs/llm/` — SSOT, max 2,000 tokens (pages/ max 1,500) |
| No orphan docs | Every doc must be indexed in `docs/llm/README.md` |
| Constitutional gate | New policy/security/identity rules → flag for human approval before merging |
| No speculation | Only document what was actually built and verified |
