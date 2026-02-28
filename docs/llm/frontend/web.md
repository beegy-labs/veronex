# Web — Brand, Design System & Architecture

> SSOT | **Last Updated**: 2026-02-28 (provider taxonomy, power comparison, test API keys)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new data table | `web/components/data-table.tsx` (SSOT) | Use `<DataTable minWidth="...">` — never write raw `<Card><CardContent p-0 overflow-x-auto><Table>` |
| Add new nav link | `web/components/nav.tsx` `navItems` array + `web/messages/en.json` `nav.*` | Add item + i18n key in all 3 locales |
| Add new color token | `web/app/tokens.css` | Layer 1 (`--palette-*`) → Layer 2 (`--theme-*`) → Layer 0 (`@property`) → Layer 3 (`@theme inline`) |
| Add new locale | `web/i18n/config.ts` `locales[]` + new `web/messages/{locale}.json` + `language-switcher.tsx` | Copy en.json structure, translate values |
| Add new provider backend type | See "Adding a New Provider" section below | 5-step process: nav → page → i18n → Rust adapter → docs |
| Change nav collapsed localStorage key | `web/components/nav.tsx` `localStorage('nav-collapsed')` | Change key string (clears all users' preferences) |
| Change theme colors | `web/app/tokens.css` Layer 2 `--theme-*` values | Only edit `--theme-*` variables, never hardcode hex in TSX |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/tokens.css` | Design token SSOT (4-layer architecture) |
| `web/app/globals.css` | Tailwind v4 entry + focus ring |
| `web/app/layout.tsx` | `ThemeProvider` + `I18nProvider` + `QueryClientProvider` |
| `web/components/nav.tsx` | Collapsible sidebar + `HexLogo` SVG |
| `web/components/theme-provider.tsx` | `data-theme` switcher, `localStorage('hg-theme')` |
| `web/components/i18n-provider.tsx` | react-i18next wrapper |
| `web/i18n/config.ts` | `locales[]`, `localeLabels{}`, `defaultLocale` |
| `web/i18n/index.ts` | i18next init |
| `web/messages/en.json` | Source of truth for all i18n keys |
| `web/components/data-table.tsx` | `DataTable` + `DataTableEmpty` — SSOT for all data tables |
| `web/lib/chart-theme.ts` | Recharts style constants SSOT (`TOOLTIP_STYLE`, `AXIS_TICK`, `LEGEND_STYLE`, …) |
| `web/components/donut-chart.tsx` | Shared `DonutChart` component — always use instead of inline `<PieChart>` |
| `web/lib/api.ts` | All API client functions |
| `web/lib/types.ts` | All TypeScript types |
| `web/package.json` | Next.js 15, Tailwind v4, TanStack Query, shadcn/ui |

---

## Brand (Veronex)

- **Name**: Vero (truth/precision) + Nexus (connection hub)
- **Logo**: `HexLogo` component in `nav.tsx` — flat-top honeycomb hexagon SVG, 32×32 viewBox
- **Logo CSS vars**: `var(--theme-logo-start)`, `var(--theme-logo-end)`
- **Favicon**: `web/public/favicon.svg` — forest gradient `#0d2518 → #16402e`
- **Wordmark**: `web/public/logo.svg` — hex mark + "Veronex" text in `#16402e`
- **Dark mode logo**: violet gradient `#a78bfa → #c4b5fd` (unchanged)

---

## Design Theme — "Verde Nexus"

| | Light "Platinum Signal" | Dark "Obsidian Verde" |
|---|---|---|
| Page bg | `#f2f4f2` Platinum Pearl | `#080a09` Obsidian Deep |
| Card bg | `#ffffff` Pure White | `#111412` Dark Graphite |
| Primary | `#0f3325` Deep Ivy (12.71:1 AAA) | `#10b981` Bio-Emerald (7.73:1 AAA) |
| Text primary | `#141a14` Anthracite ~14.4:1 AAA | `#e2e8e2` Soft Platinum ~14.2:1 AAA |
| Text secondary | `#334155` Slate Silver ~10:1 AAA | `#94a3b8` Titanium Silver ~7.7:1 AAA |
| Border | `#e2e8e0` | `#1a2118` |
| Button fg | `#ffffff` on Deep Ivy | `#041f16` Deep Dark on Bio-Emerald |

WCAG targets: Primary ≥7:1 (AAA), body text AAA, status colors AAA both modes.
Light logo: `#091e12 → #0f3325` · Dark logo: `#047857 → #10b981` (emerald gradient)
Dark status colors: `#34d399` / `#fb7185` / `#fbbf24` / `#60a5fa`

---

## tokens.css — 4-Layer Token Architecture

```css
/* Layer 0: @property — type safety + CSS transition support */
@property --theme-primary { syntax: '<color>'; ... }

/* Layer 1: --palette-* raw hex (NEVER use in components) */
--light-primary: #16402e;   /* Deep Forest — 11.45:1 on white ✓ AAA */

/* Layer 2: --theme-* semantic (switches via [data-theme='dark']) */
--theme-primary: var(--light-primary);          /* light */
[data-theme='dark'] --theme-primary: ...;        /* dark */

/* Layer 3: @theme inline — Tailwind utility generation */
@theme inline { --color-primary: var(--theme-primary); }
```

**Token flow for new tokens**: Layer 1 → Layer 2 → Layer 0 → Layer 3.

---

## Key Policies

| Policy | Rule |
|--------|------|
| Color | Zero hardcoded hex in TSX. Use Tailwind utilities or `var(--theme-*)` |
| Headings | `text-2xl font-bold tracking-tight` |
| Status order | Always: pending → running → completed → failed → cancelled |
| i18n | All user-visible strings via `t('key')` — no hardcoded English |
| Terminology | See [`docs/llm/policies/terminology.md`](../policies/terminology.md) — SSOT for all term definitions |
| Recharts | Import from `web/lib/chart-theme.ts` (SSOT) — never define chart constants in page files. See `frontend/web-charts.md` |
| Focus ring | `4px solid var(--theme-focus-ring)`, offset 4px |
| Font | System font stack only — no Google Fonts (breaks CJK) |

---

## Nav Sidebar (nav.tsx)

```
Overview          → /overview
Jobs              → /jobs
API Keys          → /keys
Servers           → /servers           ← standalone link (HardDrive icon)
▼ Providers       ← collapsible group (Server icon)
  ├── Ollama      → /providers?s=ollama
  └── Gemini      → /providers?s=gemini
Usage             → /usage
Performance       → /performance
Test              → /api-test
API Docs          → /api-docs

Footer: v0.1.0 · [🌐 EN ▾] · [☀/🌙]
```

- Width: `w-56` expanded / `w-14` collapsed; `transition-all duration-200`
- Collapse state: `localStorage('nav-collapsed')`
- Group state: `localStorage('nav-group-{id}')`, auto-open on active route
- `NavContent` (uses `useSearchParams`) wrapped in `<Suspense>` in outer `Nav`
- Servers: top-level `NavLink` at `/servers` (no sub-items)
- Providers: `NavGroup` with `id: 'providers'`, `basePath: '/providers'`

### Mobile Responsive Nav (hamburger slide sidebar)

On `< md` breakpoint the sidebar is hidden. A fixed top bar replaces it:

```
Mobile (closed):
┌────────────────────────┐
│ ☰  [hex] Veronex       │  ← fixed top bar, h-12, z-30
├────────────────────────┤
│       Content          │  ← pt-16 to clear top bar
└────────────────────────┘

Mobile (open):
┌──────────┬─────────────┐
│ w-72     │ dimmed bg   │  ← aside z-50, backdrop z-40
│ Sidebar  │             │
└──────────┴─────────────┘
```

- Mobile top bar: `md:hidden fixed top-0 left-0 right-0 z-30 h-12 bg-card border-b`
- Sidebar: `fixed inset-y-0 left-0 z-50 w-72` → `transition-transform` slide in/out
- Backdrop: `fixed inset-0 z-40 bg-black/50` — click to close
- Desktop override: `md:static md:z-auto md:translate-x-0` (back to flex child)
- Auto-close on route change (`useEffect` on `pathname`)
- `layout.tsx` main: `p-4 pt-16 md:p-8` (clears mobile top bar)

---

## Data Tables — `DataTable` Component (SSOT)

**All data tables in the app use `<DataTable>`** — `web/components/data-table.tsx`.

```tsx
import { DataTable, DataTableEmpty } from '@/components/data-table'
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

// Standard table
<DataTable minWidth="700px">
  <TableHeader>
    <TableRow>
      <TableHead>Name</TableHead>
      <TableHead className="text-right">Actions</TableHead>
    </TableRow>
  </TableHeader>
  <TableBody>...</TableBody>
</DataTable>

// With pagination footer
<DataTable minWidth="700px" footer={totalPages > 1 ? <PaginationRow /> : undefined}>
  ...
</DataTable>

// Empty state
<DataTableEmpty>{t('common.noData')}</DataTableEmpty>
```

### DataTable Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `minWidth` | `string` | `'600px'` | Minimum table width before horizontal scroll |
| `footer` | `ReactNode` | — | Optional footer inside the Card (e.g. pagination) |

### base `table.tsx` padding (SSOT)

`TableHead`: `h-11 px-4`, first cell `pl-6`, last cell `pr-6`
`TableCell`: `py-3 px-4`, first cell `pl-6`, last cell `pr-6`

**Rule**: never override `pl-*`/`pr-*` on first/last cells — edge padding is the base component's responsibility.

### min-width reference

| Page / Component | minWidth |
|-----------------|----------|
| `servers/page.tsx` ServersTable | `700px` |
| `providers/page.tsx` OllamaTab | `800px` |
| `providers/page.tsx` GeminiTab backends | `760px` |
| `providers/page.tsx` GeminiTab policy table | `600px` |
| `keys/page.tsx` | `700px` |
| `components/job-table.tsx` | `760px` |

> **Rule**: When adding a new table, use `<DataTable minWidth="...">` — never write `<Card><CardContent className="p-0 overflow-x-auto"><Table className="min-w-...">` directly. All Card + scroll boilerplate lives in `DataTable`.

---

## State Management

- Server state: TanStack Query (`useQuery`, `useMutation`)
- Query keys: `['backends']`, `['servers']`, `['gemini-policies']`, `['gemini-models']`,
  `['gemini-sync-config']`, `['job-detail', jobId]`, etc.
- Local state: `useState` for modals
- No global client store (no Redux/Zustand)

---

## i18n

- 3 locales: `en` (default), `ko`, `ja`
- Detection: `localStorage('hg-lang')` → `navigator.language` → `'en'`

### Adding i18n Keys

1. Add key to `web/messages/en.json` (source of truth)
2. Add to `web/messages/ko.json` (Korean)
3. Add to `web/messages/ja.json` (Japanese)
4. Use: `const { t } = useTranslation()` → `t('section.key')`

---

## Overview Page (`/overview`)

Integrated system health dashboard — answers "is the system healthy?" at a glance.

### Data Fetches (all parallel, graceful degradation)

| Query | Source | refetch | Notes |
|-------|--------|---------|-------|
| `api.stats()` | PostgreSQL | 30s | includes `test_keys` count |
| `api.backends()` | PostgreSQL | 30s | |
| `api.servers()` | PostgreSQL | 60s, retry:false | |
| `useQueries` per server → `api.serverMetrics(id)` | node-exporter | 30s, retry:false | live W |
| `useQueries` per server → `api.serverMetricsHistory(id, 1440)` | ClickHouse | stale 5m, retry:false | 60-day kWh history |
| `api.performance(24)` | ClickHouse | 60s, retry:false | |
| `api.usageAggregate(24)` | ClickHouse | 60s, retry:false | |
| `api.usageBreakdown(24)` | ClickHouse | 60s, retry:false | provider per model |
| `api.jobs('limit=10')` | PostgreSQL | 30s | |

ClickHouse-dependent values show `"—"` when ClickHouse is offline (graceful degradation).

### Power Calculation (frontend-only)

```ts
// Live watt sum across all servers
const totalPowerW = serverMetricQueries.reduce((sum, q) =>
  sum + q.data?.gpus.reduce((gs, g) => gs + (g.power_w ?? 0), 0), 0)

// Actual kWh from history — hours=1440 returns 60-min buckets → 1 point = 1 kWh/kW
function sumKwhInWindow(fromHoursAgo: number, toHoursAgo: number): number {
  const now     = Date.now()
  const startMs = now - fromHoursAgo * 3_600_000
  const endMs   = now - toHoursAgo   * 3_600_000
  // sum gpu_power_w / 1000 for all points within [startMs, endMs)
}

// fromHoursAgo MUST be > toHoursAgo (older bound first)
const kwhThisWeek  = sumKwhInWindow(168, 0)    // or projected from live W if no history
const kwhLastWeek  = sumKwhInWindow(336, 168)
const weekDelta    = kwhThisWeek - kwhLastWeek  // positive = more than last week (warning)

const kwhThisMonth = sumKwhInWindow(720, 0)
const kwhLastMonth = sumKwhInWindow(1440, 720)
const monthDelta   = kwhThisMonth - kwhLastMonth
```

### Provider Taxonomy

Backends are grouped into two **generic categories** (future-proof):

| Category | i18n key | Icon | `backend_type` values | Examples |
|----------|----------|------|----------------------|---------|
| **Local** | `overview.localProviders` | `Server` | `['ollama']` | Ollama, vLLM, LocalAI |
| **API Services** | `overview.apiProviders` | `Globe` | `['gemini']` | Gemini, OpenAI, Anthropic |

```ts
const LOCAL_TYPES = ['ollama'] as const   // extend when adding self-hosted backends
const API_TYPES   = ['gemini'] as const   // extend when adding cloud API backends
const localBs = backends?.filter(b => LOCAL_TYPES.includes(b.backend_type)) ?? []
const apiBs   = backends?.filter(b => API_TYPES.includes(b.backend_type))   ?? []
```

**Rule**: Never hard-code "Ollama" or "Gemini" labels in Overview. Use `localProviders`/`apiProviders` i18n keys.

### Layout (7 sections)

```
Header: h1 "Overview" + subtitle "System health at a glance"

Section 1 — System KPIs (grid-cols-2 sm:grid-cols-3 xl:grid-cols-5)
  [Providers N/M online] [In Queue N] [24h Requests] [Success %] [P95 Latency]
  - Providers: online count / total backends
  - P95: fmtMs() — auto-converts to s/m/h

Section 2 — Infrastructure (grid-cols-2 sm:grid-cols-4)
  [GPU Servers: N registered, N live]
  [GPU Power: X.XX kW]
  [Weekly kWh: X.X kWh | ±delta vs prev week]   ← actual history, colored delta
  [Monthly kWh: X.X kWh | ±delta vs prev month]  ← actual history, colored delta
  - delta > 0 (higher consumption): text-status-warning-fg
  - delta < 0 (lower consumption):  text-status-success-fg
  - No history: shows live-W projection, static "prev week/month" label

Section 3 — Provider Status + API Keys (grid-cols-1 md:grid-cols-2)
  Left: "Provider Status" card
    Server icon · "Local" row:        N online · N degraded · N offline
    Globe icon  · "API Services" row: N online · N degraded · N offline
    Footer: "View Providers →" → /providers
  Right: "API Keys" card
    active_keys (standard) · test_keys (shown when > 0, blue)
    Footer: "View Keys →" → /keys

Section 4 — Request Trend (full-width AreaChart)
  Total + Success from perf.hourly; hidden if empty

Section 5 — Top Models (full-width horizontal BarChart)
  Data: breakdown.by_model sorted desc by request_count, top 8
  Bar: Cell per bar — Local=var(--theme-primary), API=var(--theme-status-info)
  Y-axis: model_name (width=154, max 22 chars + "…")
  Tooltip: shows backend name

Section 6 — Recent Jobs (full-width Card)
  Mini table: Model | Provider | Status | Latency | Created (10 rows)

Section 7 — Token Summary + Performance (grid-cols-1 md:grid-cols-2)
```

### i18n Keys (overview.*)

Base keys: `providerStatus`, `queueDepth`, `recentJobs`, `viewAllJobs`,
`tokenSummary`, `perfSummary`, `goToProviders`, `goToKeys`, `goToUsage`, `goToPerformance`

Infrastructure / provider keys:
`infrastructure`, `gpuPower`, `gpuServers`, `weeklyPower`, `monthlyPower`,
`prevWeek`, `prevMonth`, `topModels`, `noServerPower`,
`localProviders`, `apiProviders`, `testKeys`, `activeKeysLabel`

---

## Duration / Latency Formatter — `fmtMs` (SSOT)

Shared in `web/lib/chart-theme.ts`. Use everywhere — never inline ms conversion.

| Function | Use case | Example |
|----------|----------|---------|
| `fmtMs(n)` | KPI cards, tooltips, table cells | `86360` → `"1m 26s"` |
| `fmtMsAxis(n)` | Chart Y-axis tick labels (compact) | `86360` → `"1.4m"` |
| `fmtMsNullable(n)` | Nullable job latency | `null` → `"—"` |

Tiers: `< 1s` → `"Xms"` · `1s–59s` → `"X.Xs"` · `1m–59m` → `"Xm Xs"` · `≥ 1h` → `"Xh Xm"`

**Rule**: All latency display in the app uses these functions. Never write `${n}ms` or `${n/1000}s` directly in TSX.

---

## Adding a New Provider (e.g. OpenAI)

1. Add entry to `navItems[].children` in `nav.tsx` (under `providers` group)
2. Add `section === 'openai'` branch in `providers/page.tsx` → new `<OpenAITab>`
3. Add i18n key `nav.openai` + tab strings to all 3 message files
4. Extend `BackendType` enum in Rust + add adapter in `infrastructure/outbound/`
5. Update `docs/llm/backend/backends-ollama.md` + `docs/llm/backend/openai.md`
6. Create `docs/llm/frontend/web-providers.md` section for the new tab
