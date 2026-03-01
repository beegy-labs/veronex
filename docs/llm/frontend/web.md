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

## Usage Page (`/usage`)

Token + request consumption analytics with time-range selector.

### Queries (all parallel, ClickHouse graceful degradation)

| Query | refetch | Notes |
|-------|---------|-------|
| `api.usageAggregate(hours)` | 60s | aggregate KPIs |
| `api.analytics(hours)` | 60s | model distribution bar chart |
| `api.performance(hours)` | 60s, retry:false | global trend via `perf.hourly` |
| `api.usageBreakdown(hours)` | 60s | provider / key / model breakdown |
| `api.keys()` | stale 120s | key selector for per-key hourly chart |
| `api.keyUsage(keyId, hours)` | 60s, enabled when keyId set | per-key hourly tokens/requests |

### Time Range

```ts
const TIME_OPTIONS = [
  { label: '24h', hours: 24 },
  { label: '7d',  hours: 168 },
  { label: '30d', hours: 720 },
]
```

Buttons in header: selected = `variant="default"`, others = `variant="outline"`.

### Layout (top to bottom)

```
Header + [24h] [7d] [30d] time-range buttons

[ClickHouse unavailable banner — only when error]

KPI row (grid-cols-2 xl:grid-cols-4):
  [Total Requests] [Total Tokens] [Success %] [Errors / Cancelled]
  - Errors icon: AlertTriangle (red) when errorRate ≥ 10%, else XCircle

TokenDonut card (prompt vs completion split, hidden when total=0)
  - Prompt: var(--theme-primary)
  - Completion: var(--theme-status-info)

Global Trend AreaChart (hidden when no data)
  - request_count (primary) + total_tokens (info blue)
  - data: perf.hourly mapped to { hour, requests, tokens }

Model Distribution horizontal BarChart (hidden when empty)
  - data: analytics.models sorted desc, top 8
  - Bar fill: var(--theme-primary), radius: [0,4,4,0]
  - Y-axis: model name (width=150, truncated at 22 chars)

Breakdown Card (hidden when no data):
  BackendBreakdownSection  — grid-cols-1 sm:grid-cols-2 provider cards
  KeyBreakdownSection      — DataTable (minWidth=560px)
  ModelBreakdownSection    — DataTable (minWidth=600px): model+provider+req+call%+latency+tokens

Per-key Hourly card (hidden when no keys):
  Select dropdown → key selector (first key default)
  Tokens/Hour AreaChart: prompt (primary) + completion (info blue)
  Requests/Hour BarChart: requests (primary) + success (success) + errors (error)

AnalyticsSection (ClickHouse only):
  Analytics KPIs (grid-cols-3): Avg TPS | Avg Prompt Tokens | Avg Completion Tokens
  Model dist table (xl:col-span-3) + Finish Reason donut (xl:col-span-2)
```

### Backend / Finish Reason Colors (SSOT in file)

```ts
const BACKEND_COLORS = { ollama: 'var(--theme-primary)', gemini: 'var(--theme-status-info)' }
const FINISH_COLORS  = {
  stop: 'var(--theme-status-success)', length: 'var(--theme-status-warning)',
  error: 'var(--theme-status-error)', cancelled: 'var(--theme-text-secondary)',
}
```

**Rule**: extend these maps when adding new backend types — never hardcode backend names in JSX.

### i18n Keys (usage.*)

`title`, `description`, `totalRequests`, `totalTokens`, `success`, `errors`, `completed`, `cancelled`,
`noData`, `noDataHint`, `noKeyData`, `noKeysMsg`, `analyticsUnavailable`, `clickhouseDisabled`,
`analyticsTitle`, `analyticsDesc`, `avgTps`, `avgTpsDesc`, `avgPromptTokens`, `avgCompletionTokens`,
`tokensPerReq`, `byProvider`, `byKey`, `callShare`, `modelCallRatio`, `providerCol`,
`modelDistTitle`, `finishReasonTitle`, `hourly`, `tokensPerHour`, `requestsPerHour`,
`reqCount`, `successRate`, `avgLatency`, `totalTok`, `requests`, `breakdownTitle`, `breakdownDesc`,
`modelName`

---

## Performance Page (`/performance`)

Latency percentile analysis + hourly trend charts. ClickHouse required.

### Query

`api.performance(hours)` — `refetchInterval: 60_000`

Returns `{ p50, p95, p99, avg_latency_ms, success_rate, total_requests, hourly[] }`.

### Time Range

Same `TIME_OPTIONS` as Usage: `24h / 7d / 30d`.

### Layout

```
Header + [24h] [7d] [30d] buttons

[ClickHouse unavailable banner]
[No data state when total_requests = 0]

KPI cards (grid-cols-3 sm:grid-cols-5):
  [P50] [P95] [P99] [Success Rate] [Error Count]

Latency Percentiles detail card (grid-cols-2 sm:grid-cols-4):
  P50 | P95 | P99 | Avg — each in a sub-card with centered large mono value

Avg Latency/Hour LineChart:
  - Y-axis: fmtMsAxis()
  - ReferenceLine at p95_latency_ms (dashed, warning color, label "P95")
  - Tooltip: fmtMs()

Throughput/Hour BarChart:
  - total (primary) + success (success green) + errors (error red)
  - Header shows error count badge when > 0

Error Rate/Hour LineChart:
  - dataKey="errorRate" (derived: (total-success)/total*100)
  - Y-axis domain [0,100], tickFormatter: v => `${v}%`
  - Tooltip: formatter shows percentage
  - Line color: var(--theme-status-error)
```

### Chart Data Derivation

```ts
const chartData = data?.hourly.map((h) => ({
  hour:      fmtHour(h.hour),
  latency:   Math.round(h.avg_latency_ms),
  total:     h.request_count,
  success:   h.success_count,
  errors:    Math.max(0, h.request_count - h.success_count),
  errorRate: h.request_count > 0
    ? parseFloat(((h.request_count - h.success_count) / h.request_count * 100).toFixed(1))
    : 0,
}))
```

### i18n Keys (performance.*)

`title`, `description`, `p50`, `p95`, `p99`, `avgLatency`, `successRate`, `errors`,
`latencyPercentiles`, `aggregatedOver`, `avgLatencyHour`, `throughputHour`,
`analyticsUnavailable`, `clickhouseDisabled`, `noData`, `noDataHint`,
`errorRate`, `errorRateTrend`

---

## Jobs Page (`/jobs`)

Paginated job history with search, status filter, and embedded test runner.

### Tabs

| Tab | `source` param | Description |
|-----|---------------|-------------|
| "API Jobs" | `source=api` | Jobs from production API keys |
| "Test Runs" | `source=test` | Jobs triggered via the test panel |

### JobsSection (reusable component)

Props: `{ source: 'api' | 'test', onRetry?: (params: RetryParams) => void }`

Query: `api.jobs(URLSearchParams{ limit, offset, source, status?, q? })` — `refetchInterval: 30_000`

Controls:
- Search input (Enter = commit, Esc = clear) — queries against model name / prompt
- Status filter Select: `all | pending | running | completed | failed | cancelled`
- Pagination: `PAGE_SIZE = 50`, smart page slot renderer (max 7 slots, `…` ellipsis)

### Retry Flow

`JobTable` row has a retry button that calls `onRetry(RetryParams)`.
`onRetry` in `JobsPage`:
1. Sets `retryParams` state
2. Switches active tab to `'test'`
3. Scrolls to `testPanelRef` (smooth scroll, 50ms delay)
`ApiTestPanel` receives `retryParams` prop and pre-fills the form.

### JobTable (`web/components/job-table.tsx`)

- `minWidth="760px"` via `DataTable`
- Columns: Model · Provider · Status · Prompt Tokens · Completion Tokens · Latency · Created · Actions
- Row click → `JobDetailModal` (detail + prompt preview)
- Status badge colors: same `STATUS_EXTRA` map as Overview
- Retry button: appears on `completed` / `failed` rows

### i18n Keys (jobs.*)

`title`, `description`, `apiJobs`, `testRuns`, `allStatuses`, `statuses.*`,
`totalLabel`, `searchPlaceholder`, `searchingFor`, `clearSearch`,
`noJobs`, `loadingJobs`, `failedJobs`, `model`, `backend`, `status`, `latency`, `createdAt`

---

## Keys Page (`/keys`)

API key management: create standard / test keys, toggle active, delete.

### Queries

| Query | refetch |
|-------|---------|
| `api.keys()` | 60s |

### Mutations

- `api.createKey({ name, tenant_id, rate_limit_rpm?, rate_limit_tpm?, key_type })` → `CreateKeyResponse`
- `api.deleteKey(id)`
- `api.toggleKeyActive(id, is_active)`

### Key Types

| `key_type` | UI badge | Use |
|-----------|----------|-----|
| `'standard'` | plain text label | Production API access |
| `'test'` | `FlaskConical` + info-blue badge | Safe testing; counted separately in Overview stats |

### Layout

```
Header:
  h1 "API Keys" + subtitle (N keys)
  Buttons: [Flask Test Key] [+ Create Key]

DataTable (minWidth="700px"):
  Name | Prefix | Tenant | Type | Status | Toggle | RPM / TPM | Created | Actions

Modals:
  CreateKeyModal    — name (required) + tenant_id + RPM + TPM + key_type prop
  KeyCreatedModal   — shows raw key once (copy button), warning banner
  DeleteConfirmModal — confirm by name
```

### Type Badge Colors

- `standard`: `<span className="text-xs text-muted-foreground">` (label via `overview.activeKeysLabel`)
- `test`: `<Badge>` with `bg-status-info/10 text-status-info-fg border-status-info/30`

### Rate Limit Display

`rate_limit_rpm === 0` → `∞`; same for `tpm`.

### i18n Keys (keys.*)

`title`, `name`, `prefix`, `tenant`, `type`, `status`, `activeToggle`, `rpmTpm`, `createdAt`, `actions`,
`keyName`, `keyNamePlaceholder`, `tenantId`, `rateLimitRpm`, `rateLimitTpm`, `rateLimitPlaceholder`,
`createKey`, `createTestKey`, `createTitle`, `createTestTitle`, `creating`, `nameTaken`,
`createdTitle`, `createdWarning`,
`deleteTitle`, `deleteConfirm`, `deleteKey`, `deleting`,
`loadingKeys`, `failedKeys`, `noKeys`

---

## Servers Page (`/servers`)

GPU server registration and live metrics management.

### Query & Mutations

| Operation | API | Notes |
|-----------|-----|-------|
| `api.servers()` | GET | refetchInterval: 30s |
| `api.registerServer({ name, node_exporter_url? })` | POST | invalidates `['servers']` |
| `api.updateServer(id, { name?, node_exporter_url? })` | PATCH | invalidates `['servers']` |
| `api.deleteServer(id)` | DELETE | `window.confirm()` before firing |

### Layout

```
Header: h1 "GPU Servers" + subtitle

ServersTable:
  Status pill bar + [+ Register Server] button
    Pill 1: HardDrive icon · N registered
    Pill 2: green · N with metrics (node_exporter_url configured)
    Pill 3: muted · N without exporter

  DataTable (minWidth="700px", PAGE_SIZE=10, pagination in footer):
    Name | node_exporter_url (mono code span) | Live Metrics | Registered | Actions

  Actions per row (3 icon buttons):
    BarChart2  → opens ServerHistoryModal   (accent-gpu hover)
    Pencil     → opens EditServerModal      (primary hover)
    Trash2     → confirm() → deleteMutation (error hover)

Modals:
  RegisterServerModal — name (required) + node_exporter_url (optional)
  EditServerModal     — pre-filled with current values
  ServerHistoryModal  — ClickHouse history charts (see below)
```

### ServerMetricsCell (`web/components/server-metrics-cell.tsx`)

Two exported variants, both use query key `['server-metrics', serverId]`:

| Component | Used in | Layout |
|-----------|---------|--------|
| `ServerMetricsCell` | Servers page table | Full multi-line: MEM / CPU / GPU rows |
| `ServerMetricsCompact` | OllamaTab backend row | Inline flex row: MEM % · CPU % · Temp · Power |

Both: `refetchInterval: 30_000, retry: false`

**Color thresholds (shared logic):**
- MEM%: `≥90%` → error-fg · `≥75%` → warning-fg
- CPU%: `≥90%` → error-fg · `≥70%` (Compact) / `≥75%` (Full) → warning-fg
- GPU temp: `≥85°C` → error-fg · `≥70°C` (Compact only) → warn-fg

When `scrape_ok = false`: shows `WifiOff` badge + `RefreshCw` retry button (Full variant).

### ServerHistoryModal (`web/components/server-history-modal.tsx`)

Query: `api.serverMetricsHistory(server.id, hours)` — staleTime: 0 (always fresh on open)

Time range tabs: `[1h] [3h] [6h] [24h]` (HIST_HOUR_OPTIONS constant)

Charts (only shown when data exists):
1. **Mem Used %** — LineChart, Y [0–100], color `var(--theme-status-info)`
2. **GPU Temp °C** — LineChart, `connectNulls`, color `var(--theme-status-error)` (hidden if no GPU data)
3. **GPU Power W** — LineChart, `connectNulls`, color `var(--theme-accent-power)` (hidden if no GPU data)

Sync button (RefreshCw) in header — re-fetches current range.

### i18n Keys (backends.servers.*)

`title`, `description`, `registered`, `withMetrics`, `noExporter`, `registerServer`,
`registerTitle`, `editTitle`, `name`, `nodeExporterUrl`, `nodeExporterOptional`,
`nodeExporterUrlPlaceholder`, `nodeExporterHint`,
`liveMetrics`, `registeredAt`, `history`, `unreachable`,
`loadingServers`, `noServers`, `noServersHint`, `notConfigured`

Also referenced: `backends.clickhouseHistory`, `backends.noClickhouseData`, `backends.checkOtel`,
`backends.memUsedPct`, `backends.gpuTempC`, `backends.gpuPowerW`

---

## Providers Page (`/providers`)

Backend management for all inference providers. Routing: `/providers?s=ollama` (default) / `?s=gemini`.

`useSearchParams()` is in `NavContent` (inner component wrapped in `<Suspense>`).

### Shared Queries (fetched at page level, passed as props to tabs)

| Query key | Fetches | refetch |
|-----------|---------|---------|
| `['backends']` | `api.backends()` | 30s |
| `['servers']` | `api.servers()` | 60s |

### Shared Mutations (wired at page level, passed as props)

| Mutation | API call |
|----------|----------|
| Register | `api.registerBackend(body)` |
| Healthcheck | `api.healthcheckBackend(id)` |
| Delete | `api.removeBackend(id)` |
| Sync models (Ollama) | `api.syncBackendModels(id)` |
| Toggle active (Gemini) | `api.toggleBackendActive(id, is_active)` |

### Shared Components

**`StatusBadge`** — `online` (success green) / `degraded` (warning amber) / `offline` (muted)

**`VramInput`** — inline MiB/GiB unit toggle; converts to MB internally; used in Register & Edit modals.

**`EditModal`** — shared for both Ollama and Gemini. Adapts fields by `backend.backend_type`:
- Ollama: name + URL + GPU Server Select + GPU Index (dynamic from live metrics) + VRAM (VramInput)
- Gemini: name + API Key (password, empty = keep existing) + `is_free_tier` Switch

**`RegisterModal`** — same dual-mode as EditModal, prop `initialType: 'ollama' | 'gemini'`.

**`ApiKeyCell`** — reveal/hide toggle; on reveal calls `api.backendKey(id)` (enabled: false, fetch on demand).

---

### OllamaTab (`?s=ollama`)

#### Status pills

```
[Server icon · N registered] [● N online] [● N degraded] [● N offline]
```

#### OllamaTab Table (`minWidth="800px"`, PAGE_SIZE=10)

| Column | Content |
|--------|---------|
| Name | `b.name` (bold) + hostname from `b.url` (mono, dim) |
| Server | `linkedServer.name` if linked, else italic "no server linked" |
| Live Metrics | `ServerMetricsCompact` (MEM/CPU/Temp/Power inline) — only if server linked |
| Status | `StatusBadge` |
| Registered | `toLocaleDateString` |
| Actions | Healthcheck (Wifi) · Model list (⊞) · History (BarChart2) · Edit (Pencil) · Delete (Trash2) |

#### OllamaSyncSection

- **Sync All** button → `POST /v1/ollama/models/sync` — shows progress `done/total` while running
- Polling: `refetchInterval` switches to 2000ms when `syncJob.status === 'running'`, else `false`
- Model grid: searchable list of `OllamaModelWithCount` — click model → `OllamaModelBackendsModal`

#### OllamaBackendModelsModal
Shows models registered on a specific backend (from DB). Searchable badges, staleTime: 30s.

#### OllamaModelBackendsModal
Shows all backends that have a given model. Searchable + paginated (PAGE_SIZE=8). Status dot per row.

---

### GeminiTab (`?s=gemini`)

#### Status pills (4 types)

```
[Key icon · N registered] [shield · N active] [● N online] [● N degraded] [● N offline]
```

#### GeminiTab Table (`minWidth="760px"`, PAGE_SIZE=10)

| Column | Content |
|--------|---------|
| Name | `b.name` bold |
| API Key | `ApiKeyCell` — masked prefix, reveal/hide button |
| Free Tier | badge when `is_free_tier` |
| Status | `StatusBadge` |
| Active | `Switch` → `toggleBackendActive` |
| Registered | date |
| Actions | Healthcheck · Model selection (ListFilter) · Edit · Delete |

#### ModelSelectionModal (Gemini)
- Fetches `api.getSelectedModels(backend.id)` → list of `BackendSelectedModel`
- Each row: `model_name` + Switch (`is_enabled`)
- Uses optimistic mutation (`onMutate` + `onError` rollback)
- Source: global `gemini_models` pool + `backend_selected_models` overrides

---

### GeminiSyncSection

Global model pool management. Three sub-queries:

| Query key | Fetches |
|-----------|---------|
| `['gemini-sync-config']` | masked global sync API key |
| `['gemini-models']` | global model list from `gemini_models` table |
| `['gemini-policies']` | rate limit policies array |

**Sync Key**: set/edit via `SetSyncKeyModal` → `api.setGeminiSyncConfig(key)`.
**Sync Now**: `api.syncGeminiModels()` → populates `gemini_models`.
**Rate Limit Policies table** (`minWidth="600px"`):
- Columns: Model · Free Tier · RPM · RPD · Updated · Edit
- `policyMap.get('*')` = global default; per-model rows show `opacity-60` when inherited
- `EditPolicyModal`: `available_on_free_tier` Switch + RPM/RPD inputs (shown only when free tier enabled)

### GeminiStatusSyncSection

Point-in-time healthcheck for all Gemini backends. Single mutation `api.syncGeminiStatus()`.
Results: status dot + name + status label + error message per backend.

---

### i18n Keys

`backends.ollama.*`: `name`, `registerBackend`, `registerTitle`, `editTitle`, `server`, `status`,
`ollamaUrl`, `gpuServer`, `gpuServerHint`, `gpuIndex`, `maxVram`, `serverRam`, `noneOption`,
`noServerLinked`, `noBackends`, `noBackendsHint`, `loadingBackends`, `failedBackends`,
`ollamaSyncSection`, `ollamaSyncAll`, `ollamaSyncing`, `ollamaSyncDone`, `ollamaNoSync`,
`ollamaAvailableModels`, `ollamaSearchModels`, `noModelsMatch`, `noBackendModels`,
`noBackendsSynced`, `noServersMatch`, `serversWithModel`, `searchServers`,
`modelsCount`, `ollamaBackendModelsModal`

`backends.gemini.*`: `title`, `registerBackend`, `registerTitle`, `editTitle`, `apiKey`, `apiKeyHint`,
`keepExistingKey`, `freeTier`, `freeTierDesc`, `freeTierRouting`, `paidOnlyRouting`,
`model`, `modelSelection`, `modelSelectionDesc`, `noGlobalModels`, `modelsCount`,
`globalDefault`, `globalModels`, `syncSection`, `syncSectionDesc`, `syncKey`, `syncKeyHint`,
`noSyncKey`, `setSyncKey`, `syncNow`, `lastSynced`,
`rateLimitPolicies`, `rateLimitDesc`, `globalFallbackHint`, `editPolicyTitle`,
`onFreeTier`, `rpm`, `rpd`, `lastUpdated`, `availableOnFreeTier`, `enabled`, `paidOnly`,
`freeLimitsHint`, `failedToSave`,
`statusSyncSection`, `statusSyncDesc`, `syncStatus`, `syncingStatus`, `statusSyncDone`, `noStatusResults`,
`onlineCount`, `registerBackend`, `loadingBackends`, `failedBackends`

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
