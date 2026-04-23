# Frontend Audit

> ADD Execution — Frontend grep-based audit (P1/P2/P3) | **Last Updated**: 2026-04-22
> Parent: `best-practices.md` (routing + Parts 1/2). Each block notes its CDD SSOT.


Used by `code-review.md` Step 4. Run the relevant grep commands against changed files.
Each block includes `# → {doc} § {section}` — the CDD SSOT to read for the full rule + fix guidance.

### P1 — Security & Correctness (always run)

```bash
# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture) — P0, zero tolerance
grep -rn "#[0-9a-fA-F]\{3,6\}" web/app web/components --include="*.tsx" | grep -v "tokens.css\|redoc-wrapper"

# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture) — use tokens.* in inline styles
grep -rn "var(--theme-" web/app web/components --include="*.tsx"

# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture) — P0, bypasses theme
grep -rn "text-gray-\|bg-gray-\|text-slate-\|bg-slate-\|text-zinc-\|bg-zinc-" web/app web/components --include="*.tsx"

# → patterns-frontend/i18n.md § i18n Compliance — JSX text content
grep -rn '>[A-Z][a-z]' web/app web/components --include="*.tsx" | grep -v "//\|t(\|{t\|placeholder\|aria-label"
# → patterns-frontend/i18n.md § i18n Compliance — placeholder= props
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

# → patterns-frontend/queries.md § TanStack Query v5 / Mutation -- onSettled for cache invalidation
grep -rn "onSuccess.*invalidate\|onSuccess.*queryClient" web/app web/components --include="*.tsx"

# → design-system-components-patterns.md § ConfirmDialog
grep -rn "\bconfirm(" web/app web/components --include="*.tsx"

# → patterns-frontend/architecture.md § 4-Layer Component Architecture / Non-Goals — Atomic Design rejected
find web/components web/app -type d \( -name atoms -o -name molecules -o -name organisms -o -name templates \) 2>/dev/null

# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture) — colors only in tokens.css
grep -rln "^\s*--[a-z][a-z0-9-]*-color\|^\s*--palette-\|^\s*--theme-" web/ --include="*.css" --include="*.tsx" | grep -v "tokens.css\|globals.css"

# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture) — must be dual
grep -n "data-theme='dark'\]\|data-theme=\"dark\"\]" web/app/tokens.css | grep -v "\.dark"

# → patterns-frontend/tokens.md § Design Token System (4-Layer Architecture)
grep -rn "bg-\[#\|text-\[#\|border-\[#\|fill-\[#\|stroke-\[#" web/app web/components --include="*.tsx"
```

### P2 — Architecture & Performance (run if touching infra/handlers/queries)

```bash
# → patterns-frontend/architecture.md § 4-Layer Component Architecture / Violations — single-importer shared
# Shared component imported by exactly one ROUTE = candidate to move to feature.
# Excludes layout.tsx importers (those are globally shared via layout wrap).
# Use quote-delimited match to avoid \b false-positives on sibling names (e.g. nav vs nav-404-context).
for f in web/components/*.tsx; do
  name=$(basename "$f" .tsx)
  routes=$(grep -rlE "from ['\"](@/components|\./|\.\./)${name}['\"]" web/app --include="*.tsx" 2>/dev/null \
    | grep -v '/layout\.tsx$' \
    | awk -F/ '{print $3}' | sort -u)
  count=$(echo "$routes" | grep -v '^$' | wc -l)
  [ "$count" = "1" ] && echo "single-importer: $f → $routes (move to app/$routes/components/ unless name is intentionally generic)"
done

# → patterns-frontend/queries.md § TanStack Query v5 / Query Timing Constants
grep -rn "staleTime:\s*[0-9]" web/lib/queries web/app web/components --include="*.ts" --include="*.tsx"

# → patterns-frontend/queries.md § TanStack Query v5 / withJitter() — Polling Storm Prevention
grep -rn "refetchInterval:\s*[A-Z_]\+" web/lib web/app --include="*.ts" --include="*.tsx" | grep -v "withJitter\|false"

# → patterns-frontend/queries.md § TanStack Query v5 / queryOptions() Factory -- SSOT Pattern
grep -rn "queryOptions({" web/app --include="*.tsx"

# → patterns-frontend/react.md § useMemo for Derived Data
grep -rn "\.filter(\|\.sort(\|\.map(" web/app web/components --include="*.tsx" | grep -v "useMemo\|useCallback\|//\|test"

# → patterns-frontend/tokens.md § Chart Theme Formatters
grep -rn "function fmt_\|const fmt_\|\.toFixed(\|toLocaleString(" web/app web/components --include="*.tsx" | grep -v "//\|chart-theme"

# → patterns-frontend/react.md § React 19 -- useOptimistic
grep -rn "<Switch" web/app web/components --include="*.tsx" | grep -v "useOptimistic"
```

### P3 — Quality (run if touching shared utilities or tests)

```bash
# → patterns-frontend/types.md § TypeScript Strictness
grep -rn ": any\b\|as any\b" web/app web/lib web/components --include="*.ts" --include="*.tsx"

# → patterns-frontend/tokens.md § Shared Style Constants — import from constants.ts, never duplicate
grep -rn "pending.*running.*completed\|completed.*failed.*cancelled" web/app web/components --include="*.tsx" | grep -v "constants\|//\|import"

# → patterns-frontend/testing.md § E2E Test Patterns / Resource Cleanup
grep -rn "api\.post\|api\.delete" web/e2e --include="*.ts" | grep -v "finally\|helpers"

# → patterns-frontend/ui.md § lucide-react v1 Patterns
grep -rn "lucide-[a-z]" web/app web/components --include="*.tsx" --include="*.css" | grep -v "//\|import"
```

---

