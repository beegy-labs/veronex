# Dependency Upgrade

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

Dependency version update request or CVE discovered. Covers both Rust crates and npm packages.

## Step 0 — Before execution: collect versions + web search

> Run this step first. The status tables below are snapshots and become stale.

### 0-A. Collect current versions

**Rust crates:**
```bash
grep -hE "^(axum|sqlx|fred|jsonwebtoken|reqwest|argon2|opentelemetry|tracing|tokio|sha2|dashmap|async-trait)" \
  crates/veronex/Cargo.toml \
  crates/veronex-mcp/Cargo.toml \
  crates/veronex-agent/Cargo.toml \
  crates/veronex-analytics/Cargo.toml \
  | sort -u
```

**npm packages:**
```bash
cat web/package.json | python3 -c "
import json,sys
d = json.load(sys.stdin)
deps = {**d.get('dependencies',{}), **d.get('devDependencies',{})}
for k,v in sorted(deps.items()): print(f'{k}: {v}')
"
```

### 0-B. Web search for latest versions

**Rust:**

| Crate | Search query |
|-------|-------------|
| opentelemetry bundle | `"opentelemetry rust crate latest stable {year}"` |
| jsonwebtoken | `"jsonwebtoken rust crate latest version {year}"` |
| axum | `"axum tokio-rs latest version {year}"` |
| sqlx | `"sqlx latest stable version {year}"` |
| fred | `"fred redis rust crate latest {year}"` |
| CVE scan | `"CVE rust axum {year}"`, `"CVE sqlx {year}"` |

**npm:** Use npm registry API — `https://registry.npmjs.org/{package}/latest` — or launch a parallel agent per package group.

### 0-C. Update status tables below, then proceed

---

## Status as of 2026-03-24

| Crate | Current | Latest stable | Status |
|-------|---------|--------------|--------|
| `opentelemetry` bundle (4 crates) | 0.31 | 0.31.x | done |
| `jsonwebtoken` | 10.3.0 | 10.3.x | done |
| `rand` | 0.9 | 0.9.x | done |
| `async-trait` | 0.1 | keep | pending — DI only (see Phase 3) |
| `axum` | 0.8 | 0.8.x | current |
| `sqlx` | 0.8 | 0.8.6 | current (0.9-alpha: watch only) |
| `fred` | 10 | 10.1.x | current |
| `reqwest` | 0.13 | 0.13.x | current |
| `tokio` | 1 | 1.x | current |
| `thiserror` | 2 | 2.x | current |

---

## Phase 3 — async-trait audit (pending)

Rule: `Arc<dyn Trait>` DI Port traits must keep `async-trait`. Concrete-type-only traits can migrate to native async fn.

```bash
grep -rn "#\[async_trait\]" crates/ | wc -l
```

All veronex Port traits use `Arc<dyn ...>` DI — most must stay. Only selectively remove where concrete types are used throughout.

---

---

## npm Frontend Status — 2026-03-25

> Major version bumps researched. New patterns documented in CDD — see links below before upgrading.

| Package | Current | Latest | Status | Migration doc |
|---------|---------|--------|--------|---------------|
| `next` | 16.2.1 | 16.2.1 | **done** | `docs/llm/frontend/design-system.md` § Next.js 16.2 |
| `react` / `react-dom` | ^19.2.0 | 19.2.4 | **done** | `docs/llm/frontend/design-system.md` § React 19.2 |
| `tailwind-merge` | ^3.5.0 | 3.5.0 | **done** | see § npm Migration Notes below |
| `lucide-react` | ^1.7.0 | 1.7.0 | **done** | see § npm Migration Notes below |
| `vitest` | ^4.1.1 | 4.1.1 | **done** | `docs/llm/policies/testing-strategy.md` § vitest v4 |
| `@tanstack/react-query` | ^5.95.2 | 5.95.2 | **done** | `docs/llm/policies/patterns-frontend.md` § TanStack Query |
| `@playwright/test` | ^1.58.2 | 1.58.2 | **done** | minor — safe bump |
| `tailwindcss` / `@tailwindcss/postcss` | ^4.2.2 | 4.2.2 | **done** | minor — safe bump |
| `i18next` | ^25.10.9 | 25.10.9 | **done** | minor — safe bump |
| `jsdom` | ^29.0.1 | 29.0.1 | **done** | test env only |
| `@types/node` | ^25 | 25.5.0 | **done** | devDep only |
| `typescript` | ^5 | 6.0.2 | **hold** | separate branch — significant |
| All `@radix-ui/*` | current | current | done | up to date |
| `zod`, `redoc`, `clsx`, `cva` | current | current | done | up to date |

### npm Migration Notes

**`tailwind-merge` 2 → 3 (MAJOR — must upgrade with Tailwind v4)**
- `cn()` / `twMerge()` call signature: **unchanged** — no code changes
- `createTailwindMerge`: add `orderSensitiveModifiers: []` field (required in v3)
- Theme key renames: `colors` → `color`, `borderRadius` → `radius`, `margin/padding/space` → `spacing`
- `validators.isLength` removed → use `validators.isNumber` + `validators.isFraction`
- `separator` config key removed; `prefix` drops trailing dash (`'tw-'` → `'tw'`)
- Already on Tailwind v4 — upgrade together

**`lucide-react` 0.x → 1.x (MAJOR)**
- **15 brand icons removed** (no aliases): `Chromium`, `Codepen`, `Codesandbox`, `Dribbble`, `Facebook`, `Figma`, `Framer`, `Github`, `Gitlab`, `Instagram`, `LinkedIn`, `Pocket`, `RailSymbol`, `Slack`, `Twitter` → replace with [Simple Icons](https://simpleicons.org/) or vendor SVGs
- `aria-hidden="true"` is now default — icon-only semantic indicators need `aria-hidden={false}` + `aria-label`
- CSS class drift: `lucide-home` → `lucide-house` (do not use `lucide-*` CSS selectors — style via `className` only)
- Import syntax: **unchanged** (`import { X } from 'lucide-react'`)
- New: `LucideProvider` for global defaults, `createLucideIcon` for custom icons
- Bundle: ~32% smaller; tree-shaking unchanged

**`vitest` 3 → 4 (MAJOR)**
- See `docs/llm/policies/testing-strategy.md` § vitest v4 Config Changes for full list
- Key: `poolOptions` → top-level `maxWorkers`; `environmentMatchGlobs` → `projects`; `test('n', fn, opts)` → `test('n', opts, fn)`
- Requires Node >= 20

**`@tanstack/react-query` 5.0 → 5.95**
- New: `mutationOptions()`, `useSuspenseQuery`, `skipToken`, `useMutationState`, `experimental_streamedQuery`, `isEnabled`
- All documented in `docs/llm/policies/patterns-frontend.md` § TanStack Query v5

### npm Upgrade Order

```
1. next 16.1.6 → 16.2.1          (pinned, manual bump — safe)
2. react / react-dom → 19.2.4    (minor — safe; update useId snapshot tests)
3. tailwind-merge 2 → 3          (with tailwindcss 4.2.2 — must be together)
4. lucide-react 0.468 → 1.7.0    (audit brand icons first; update aria-hidden)
5. vitest 3 → 4                  (update vitest.config; Node >= 20 required)
6. @playwright/test → 1.58.2     (safe minor bump)
7. @tanstack/react-query → 5.95  (safe under ^5 range; adopt new patterns)
8. jsdom 28 → 29                 (test-only; verify test suite passes)
9. @types/node 22 → 25           (devDep; verify compile)
10. typescript 5 → 6              (separate branch — assess breaking changes first)
```

---

## Rust Verification Checklist

- [ ] `cargo clippy --all-targets` — 0 warnings
- [ ] `cargo check --workspace` — compiles
- [ ] `cargo nextest run --workspace` — all pass
- [ ] Update `Last Updated` date in this file
- [ ] Mark completed items as `done` in Rust status table

## npm Verification Checklist

- [ ] `npx tsc --noEmit` — 0 errors
- [ ] `npx vitest run` — all pass
- [ ] `npx playwright test` — all pass (or `cd web && npx playwright test`)
- [ ] Visual check: icon rendering in browser (lucide rename)
- [ ] Mark completed items as `done` in npm status table

## Rules

| Rule | Detail |
|------|--------|
| One phase at a time | Verify before proceeding to next phase |
| OTel 4 crates together | Must be updated in the same commit |
| tailwind-merge + tailwindcss together | Must be in same commit (v3 only supports Tailwind v4) |
| Breaking changes first | Read migration doc / CDD section before upgrading |
| Tests must pass | Run full verification checklist after each phase |
