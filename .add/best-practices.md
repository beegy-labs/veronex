# Best Practices

> ADD Execution | **Last Updated**: 2026-04-07
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
| `docs/llm/policies/testing-strategy.md` | Test writing rules — vitest config, Playwright patterns, Rust test tools |
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
| 2 | Find violations — run audit commands from `patterns.md` (Rust) or Part 3 below (Frontend) |
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

Used by `code-review.md` Step 4. Run the relevant grep commands against changed files.
Each block includes `# → {doc} § {section}` — the CDD SSOT to read for the full rule + fix guidance.

### P1 — Security & Correctness (always run)

```bash
# → patterns-frontend.md § Design Token System (4-Layer Architecture) — P0, zero tolerance
grep -rn "#[0-9a-fA-F]\{3,6\}" web/app web/components --include="*.tsx" | grep -v "tokens.css\|redoc-wrapper"

# → patterns-frontend.md § Design Token System / Layer usage by context — use tokens.* in inline styles
grep -rn "var(--theme-" web/app web/components --include="*.tsx"

# → patterns-frontend.md § Design Token System / Layer usage by context — P0, bypasses theme
grep -rn "text-gray-\|bg-gray-\|text-slate-\|bg-slate-\|text-zinc-\|bg-zinc-" web/app web/components --include="*.tsx"

# → patterns-frontend.md § i18n Compliance — JSX text content
grep -rn '>[A-Z][a-z]' web/app web/components --include="*.tsx" | grep -v "//\|t(\|{t\|placeholder\|aria-label"
# → patterns-frontend.md § i18n Compliance — placeholder= props
grep -rn 'placeholder="[A-Za-z]' web/app web/components --include="*.tsx" | grep -v "//\|t(\|{t"

# → design-system-i18n.md — parity: en keys must exist in ko + ja
node -e "
const en = require('./web/messages/en.json');
const ko = require('./web/messages/ko.json');
const ja = require('./web/messages/ja.json');
const missing = (src, tgt, name) => {
  const flat = (o, p='') => Object.keys(o).flatMap(k => typeof o[k]==='object' ? flat(o[k], p+k+'.') : [p+k]);
  flat(src).filter(k => !flat(tgt).includes(k)).forEach(k => console.log(name+': missing '+k));
};
missing(en, ko, 'ko'); missing(en, ja, 'ja');
"

# → execution-contracts.md § Common Module Import Contract — no raw fetch, use api.ts SSOT
grep -rn "^\s*fetch(" web/app web/components --include="*.tsx" | grep -v "//\|apiFetch\|apiGet\|apiPost\|apiPut\|apiPatch\|apiDelete"

# → execution-contracts.md § Realtime Contract — no raw setInterval in components
grep -rn "setInterval(" web/app web/components --include="*.tsx" | grep -v "//\|clearInterval\|usePolling"

# → execution-contracts.md § Error Handling Contract — no (e as any).status
grep -rn "as any)\.status\|\(e as.*\)\.status" web/app web/components --include="*.tsx"

# → execution-contracts.md § Feature Boundary Rules — no cross-route imports
grep -rn "from.*app/.*components" web/app --include="*.tsx" | grep -v "own-route\|//"

# → patterns-frontend.md § TanStack Query v5 / Mutation -- onSettled for cache invalidation
grep -rn "onSuccess.*invalidate\|onSuccess.*queryClient" web/app web/components --include="*.tsx"

# → design-system-components-patterns.md § ConfirmDialog
grep -rn "\bconfirm(" web/app web/components --include="*.tsx"
```

### P2 — Architecture & Performance (run if touching infra/handlers/queries)

```bash
# → patterns-frontend.md § TanStack Query v5 / Query Timing Constants
grep -rn "staleTime:\s*[0-9]" web/lib/queries web/app web/components --include="*.ts" --include="*.tsx"

# → patterns-frontend.md § TanStack Query v5 / withJitter() — Polling Storm Prevention
grep -rn "refetchInterval:\s*[A-Z_]\+" web/lib web/app --include="*.ts" --include="*.tsx" | grep -v "withJitter\|false"

# → patterns-frontend.md § TanStack Query v5 / queryOptions() Factory -- SSOT Pattern
grep -rn "queryOptions({" web/app --include="*.tsx"

# → patterns-frontend.md § useMemo for Derived Data
grep -rn "\.filter(\|\.sort(\|\.map(" web/app web/components --include="*.tsx" | grep -v "useMemo\|useCallback\|//\|test"

# → patterns-frontend.md § Chart Theme Formatters
grep -rn "function fmt_\|const fmt_\|\.toFixed(\|toLocaleString(" web/app web/components --include="*.tsx" | grep -v "//\|chart-theme"

# → patterns-frontend.md § React 19 -- useOptimistic
grep -rn "<Switch" web/app web/components --include="*.tsx" | grep -v "useOptimistic"
```

### P3 — Quality (run if touching shared utilities or tests)

```bash
# → patterns-frontend.md § TypeScript Strictness
grep -rn ": any\b\|as any\b" web/app web/lib web/components --include="*.ts" --include="*.tsx"

# → patterns-frontend.md § Shared Style Constants — import from constants.ts, never duplicate
grep -rn "pending.*running.*completed\|completed.*failed.*cancelled" web/app web/components --include="*.tsx" | grep -v "constants\|//\|import"

# → patterns-frontend.md § E2E Test Patterns / Resource Cleanup
grep -rn "api\.post\|api\.delete" web/e2e --include="*.ts" | grep -v "finally\|helpers"

# → patterns-frontend.md § lucide-react v1 / CSS Class Name Drift
grep -rn "lucide-[a-z]" web/app web/components --include="*.tsx" --include="*.css" | grep -v "//\|import"
```

---

## Part 4 — LLM Gateway Security Audit (OWASP API + LLM 2025)

The expensive resource in an LLM gateway is GPU time and model slots, not CPU or memory.
Every check evaluates: can an attacker monopolize the GPU fleet cheaply?

### P0 — GPU Slot Monopoly / Memory DoS (always run)

```bash
# HTTP body size limit — without DefaultBodyLimit, a 500MB JSON payload is fully buffered in memory
grep -rn "DefaultBodyLimit\|RequestBodyLimitLayer" crates/veronex/src/

# max_tokens server-side cap — passing client value uncapped to upstream allows GPU monopoly
grep -rn "max_tokens" crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs | grep -v "//\|clamp\|min\|MAX"

# messages array length cap — unbounded messages array = context bomb
grep -rn "messages.*len()\|MAX_MESSAGES" crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs
```

### P1 — Slot Exhaustion / Header Hardening (run when changing infra/handlers)

```bash
# Per-key concurrent connection limit — RPM alone cannot defend against Slowloris
grep -rn "concurrent\|semaphore\|in_flight" crates/veronex/src/infrastructure/inbound/http/middleware/

# SSE streaming timeout — CancelOnDrop alone is insufficient
grep -rn "timeout\|Duration" crates/veronex/src/infrastructure/inbound/http/streaming.rs

# Response header hardening
grep -rn "nosniff\|no-store\|X-Frame-Options\|X-Content-Type" crates/veronex/src/

# Global router timeout
grep -rn "TimeoutLayer\|tower_http::timeout" crates/veronex/src/main.rs

# MCP tool call argument exfiltration — user data must not appear in outbound tool call URLs
grep -rn "format!.*namespaced\|format!.*tool_name\|format!.*args" crates/veronex/src/infrastructure/outbound/mcp/bridge.rs
```

### P2 — Defense in Depth (run during security review)

```bash
# system message override — check if client system messages can overwrite tenant prompts
grep -rn '"system"\|role.*system' crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs

# Internal error exposure — check if upstream Ollama/Gemini errors are forwarded verbatim to clients
grep -rn "e\.to_string()\|err\.to_string()\|error.*format!" crates/veronex/src/infrastructure/inbound/http/ | grep -v "//\|tracing\|warn\|debug"

# JSON injection — format!() for JSON assembly (must use serde_json::json! instead)
grep -rn 'format!.*\\"error\\"' crates/veronex/src/

# Log injection — user input interpolated via format!() in tracing fields
grep -rn 'tracing::.*format!' crates/veronex/src/
```

### Completed (reference only)

| Item | Date |
|------|------|
| SQL injection (sqlx parameterized) | baseline |
| API key hashing (BLAKE2b-256) | baseline |
| Password hashing (Argon2id) | baseline |
| SSRF defense (provider URL validation) | baseline |
| Header injection (cookie sanitize) | 2026-03-28 |
| Prompt injection (JSON safe build) | 2026-03-28 |
| XSS (mermaid SVG strip) | 2026-03-28 |
| Index naming consistency (idx_ prefix) | 2026-03-28 |
| Missing FK indexes (4 added) | 2026-03-28 |
