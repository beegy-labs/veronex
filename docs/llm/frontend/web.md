# Web — Brand, Design System & Architecture

> SSOT | **Last Updated**: 2026-03-02 (rev5: login page — remember username cookie + language selector)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new data table | `web/components/data-table.tsx` (SSOT) | Use `<DataTable minWidth="...">` — never write raw `<Card><CardContent p-0 overflow-x-auto><Table>` |
| Add new nav link | `web/components/nav.tsx` `navItems` array + `web/messages/en.json` `nav.*` | Add item + i18n key in all 3 locales |
| Add new color token | `web/app/tokens.css` | Layer 1 (`--palette-*`) → Layer 2 (`--theme-*`) → Layer 0 (`@property`) → Layer 3 (`@theme inline`) |
| Add new locale | `web/i18n/config.ts` `locales[]` + new `web/messages/{locale}.json` | Copy en.json structure, translate values; add locale→timezone default in `timezone-provider.tsx` `localeDefault()` |
| Add new provider backend type | See "Adding a New Provider" section below | 5-step process: nav → page → i18n → Rust adapter → docs |
| Add a new public (no-auth) route | `web/lib/auth-guard.ts` `PUBLIC_PATHS` array | Add path string — prevents reload loop on unauthenticated pages |
| Change nav collapsed localStorage key | `web/components/nav.tsx` `localStorage('nav-collapsed')` | Change key string (clears all users' preferences) |
| Change theme colors | `web/app/tokens.css` Layer 2 `--theme-*` values | Only edit `--theme-*` variables, never hardcode hex in TSX |
| Add a new flow visualization panel | `web/app/overview/components/` | Create panel component + update `network-flow-tab.tsx`; follow CSS Motion Path bee pattern |
| Display a new date/time field | `web/lib/date.ts` + component | Import `fmtDatetime`/`fmtDatetimeShort`/`fmtDateOnly`; call `useTimezone()` in component |
| Gate a component on a lab feature | `web/components/lab-settings-provider.tsx` | `const { labSettings } = useLabSettings(); const enabled = labSettings?.my_flag ?? false` |
| Refresh UI after lab settings change | `web/components/nav.tsx` Settings dialog toggle | After `api.patchLabSettings()` call `refetch()` from `useLabSettings()` — never re-fetch locally |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/tokens.css` | Design token SSOT (4-layer architecture) |
| `web/app/globals.css` | Tailwind v4 entry + focus ring + `@keyframes bee-fly` + `.bee-particle` |
| `web/app/layout.tsx` | `ThemeProvider` + `I18nProvider` + `TimezoneProvider` + `QueryClientProvider` + `LabSettingsProvider` |
| `web/components/lab-settings-provider.tsx` | `LabSettingsProvider` + `useLabSettings()` — SSOT for experimental feature flags; auto-fetches `GET /v1/dashboard/lab` on mount |
| `web/components/nav.tsx` | Collapsible sidebar + `HexLogo` SVG |
| `web/components/theme-provider.tsx` | `data-theme` switcher, `localStorage('hg-theme')` |
| `web/components/i18n-provider.tsx` | react-i18next wrapper |
| `web/components/timezone-provider.tsx` | `TimezoneProvider` + `useTimezone()` hook; cookie `veronex-tz` |
| `web/lib/date.ts` | Centralized date formatters (`fmtDatetime`, `fmtDatetimeShort`, `fmtDateOnly`, `fmtHourLabel`) |
| `web/i18n/config.ts` | `locales[]`, `localeLabels{}`, `defaultLocale` |
| `web/i18n/index.ts` | i18next init |
| `web/messages/en.json` | Source of truth for all i18n keys |
| `web/components/data-table.tsx` | `DataTable` + `DataTableEmpty` — SSOT for all data tables |
| `web/lib/chart-theme.ts` | Recharts style constants SSOT (`TOOLTIP_STYLE`, `AXIS_TICK`, `LEGEND_STYLE`, …) |
| `web/components/donut-chart.tsx` | Shared `DonutChart` component — always use instead of inline `<PieChart>` |
| `web/lib/auth.ts` | Token CRUD — cookie read/write/clear (access, refresh, username, role, account_id) |
| `web/lib/auth-guard.ts` | Auth flow SSOT — `PUBLIC_PATHS`, `tryRefresh()` mutex, `redirectToLogin()` |
| `web/lib/api-client.ts` | HTTP transport — attaches Bearer token, delegates 401 to auth-guard |
| `web/lib/api.ts` | All API call functions (uses apiClient) |
| `web/lib/types.ts` | All TypeScript types |
| `web/hooks/use-inference-stream.ts` | `useInferenceStream` — SSE real-time stream → `FlowEvent[]` for network flow visualization |
| `web/package.json` | Next.js 16, Tailwind v4, TanStack Query, shadcn/ui |

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
▼ Monitor         ← collapsible group (LayoutDashboard), id='overview', default OPEN
  ├── Dashboard   → /overview         (LayoutDashboard icon)
  ├── Usage       → /usage            (BarChart2 icon)
  └── Performance → /performance      (Gauge icon)
Jobs              → /jobs              ← standalone link (List icon); 3 tabs: API Jobs / Test Runs / Network Flow
API Keys          → /keys
Servers           → /servers           ← standalone link (HardDrive icon)
▼ Providers       ← collapsible group (Server icon)
  ├── Ollama      → /providers?s=ollama
  └── Gemini      → /providers?s=gemini

Footer (always visible):
  API Docs        → /api-docs          ← BookOpen icon, below auth links
  [Accounts]      → /accounts          ← JWT only
  [Audit Log]     → /audit             ← JWT + super role only
  username · logout
  v0.1.0 · [⚙️ Settings] · [☀/🌙]
```

**Settings gear (⚙️ `Settings2` icon)**: opens a Dialog with two rows:
- **Language row**: `🌐` + "Language" label + Select (en / ko / ja)
- **Timezone row**: `🕐` + "Timezone" label + Select (11 presets + "Custom…")
  - Selecting "Custom…" reveals an inline IANA input within the same dialog (no nested modal)
  - Save/Cancel buttons; Enter key also saves; validation via `isValidTimezone()`

Language and timezone selectors were moved out of the footer bar and into this dialog to prevent overflow in the narrow sidebar. The footer bar now only shows: version text | ⚙️ icon | 🌙/☀️ icon.

- Width: `w-56` expanded / `w-14` collapsed; `transition-all duration-200`
- Collapse state: `localStorage('nav-collapsed')`
- Group state: `localStorage('nav-group-{id}')`, default open for `id: 'overview'`; auto-open on active child route
- `NavContent` (uses `useSearchParams`) wrapped in `<Suspense>` in outer `Nav`
- `NavGroupChild.section` optional: if set → `?s=` query param matching; if absent → `pathname === child.href`
- `isGroupActive()` checks all children via `isChildActive()` (not `basePath.startsWith`)
- Servers: top-level `NavLink` at `/servers` (not grouped with Providers)
- Providers: `NavGroup` with `id: 'providers'`, `basePath: '/providers'`
- Monitor: `NavGroup` with `id: 'overview'`, `labelKey: 'nav.monitor'`, `basePath: '/overview'`; children: `/overview`, `/usage`, `/performance` — **`/flow` removed from nav** (accessible as Jobs page 3rd tab)
- Jobs: top-level `NavLink` at `/jobs` (standalone link, not a group)

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

### QueryClient Global Config (`web/app/layout.tsx`)

```typescript
new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
      refetchOnWindowFocus: false,  // prevent burst refetch on tab re-focus
    },
  },
})
```

`refetchOnWindowFocus: false` is critical: without it, switching browser tabs/windows triggers
simultaneous refetch of all active queries, which causes a visible "reload" effect and can race
against the token refresh mutex.

---

## Login Page (`web/app/login/page.tsx`)

Public route (`/login`) — no auth required.

### Features
- **Remember username**: checkbox "계정 저장하기" — on login success, if checked, saves username to `veronex_saved_username` cookie (30-day, `SameSite=Lax`); if unchecked, clears the cookie. On mount, pre-fills username field and auto-checks the checkbox when cookie is present.
- **Language selector**: compact `<Select>` in card footer (en / 한국어 / 日本語) — calls `i18n.changeLanguage()` + persists to `localStorage['hg-lang']`. Same mechanism as the nav Settings dialog.
- **Fully i18n**: all labels use `t('auth.*')` keys (`auth.login`, `auth.username`, `auth.password`, `auth.rememberUsername`, `auth.loginDescription`, `auth.signingIn`, `auth.invalidCredentials`).

### Cookie
| Cookie | Value | Expiry | SameSite |
|--------|-------|--------|----------|
| `veronex_saved_username` | Saved username for pre-fill | 30 days | Lax |

This cookie is **not** a session token — it only pre-fills the login form. Managed entirely in `login/page.tsx` (not in `auth.ts`).

---

## Auth — Cookie-Based Session (`web/lib/auth.ts`)

Tokens are stored in **browser cookies** (not localStorage) for persistence across browser restarts.

| Cookie | Value | Expiry |
|--------|-------|--------|
| `veronex_access_token` | JWT access token | 7 days |
| `veronex_refresh_token` | Refresh token | 7 days |
| `veronex_username` | Display name | 7 days |
| `veronex_role` | `super` \| `admin` | 7 days |
| `veronex_account_id` | UUIDv7 | 7 days |

Settings: `SameSite=Strict; path=/; expires={7d}`

**Functions** (all use `document.cookie` API directly — no external library):
```typescript
getAccessToken()  → string | null   // read cookie
getRefreshToken() → string | null
setTokens(LoginResponse)            // write all 5 cookies
setAccessToken(token)               // update access token after refresh
clearTokens()                       // delete all 5 cookies on logout
getAuthUser()     → { username, role, accountId } | null
isLoggedIn()      → boolean         // true when access_token cookie present
```

Nav sidebar reads `getAuthUser()` in a `useEffect([])` on mount to populate the user chip.

---

## Auth Guard — SSOT for Token Lifecycle (`web/lib/auth-guard.ts`)

All auth flow policy lives here. **Never** duplicate refresh or redirect logic in other files.

### Responsibility Map

| File | Owns |
|------|------|
| `auth.ts` | Token CRUD — cookie read/write/clear only |
| `auth-guard.ts` | Auth flow SSOT — mutex, refresh, redirect, public paths |
| `api-client.ts` | HTTP transport only — delegates 401 handling to auth-guard |
| `nav.tsx` logout | Calls `redirectToLogin()` — no manual `clearTokens()` + `window.location` |

### PUBLIC_PATHS

```typescript
// web/lib/auth-guard.ts
export const PUBLIC_PATHS = ['/login', '/setup'] as const
export function isPublicPath(pathname: string): boolean { ... }
```

Add any unauthenticated route here. `redirectToLogin()` is a no-op on these paths,
preventing the reload loop caused by `LabSettingsProvider` (and any other provider that
wraps the full app) calling authenticated endpoints on pages that don't require login.

### Token Refresh Mutex

```
10 queries fired simultaneously → all receive 401
  ↓
Caller 1: refreshMutex === null → set refreshMutex = doRefresh()
Callers 2–10: refreshMutex !== null → return same Promise (piggyback)
  ↓
Single POST /v1/auth/refresh sent
  ↓
Success → setAccessToken() → .finally() → refreshMutex = null
All 10: refreshed=true → retry with new token
```

```typescript
// Module-level — persists across re-renders, reset only on full page reload
let refreshMutex: Promise<boolean> | null = null
let redirecting = false

export function tryRefresh(): Promise<boolean> {
  if (refreshMutex !== null) return refreshMutex          // piggyback
  refreshMutex = doRefresh().finally(() => { refreshMutex = null })
  return refreshMutex
}

export function redirectToLogin(): void {
  if (redirecting) return                                  // already in progress
  if (isPublicPath(window.location.pathname)) return      // suppress on /login, /setup
  redirecting = true
  clearTokens()
  window.location.href = '/login'
}
```

**Module-level state** (not class-level) means the mutex survives component re-renders
and is shared across all callers in the same browser session.

---

## i18n

- 3 locales: `en` (default), `ko`, `ja`
- Labels: `en: 'English'`, `ko: '한국어'`, `ja: '日本語'`
- Detection: `localStorage('hg-lang')` → `navigator.language` → `'en'`

### Adding i18n Keys

1. Add key to `web/messages/en.json` (source of truth)
2. Add to `web/messages/ko.json` (Korean)
3. Add to `web/messages/ja.json` (Japanese)
4. Use: `const { t } = useTranslation()` → `t('section.key')`

---

## Timezone

Timezone is stored in cookie `veronex-tz` (1-year expiry, `SameSite=Lax`).

### Supported Timezones (IANA)

Preset timezones appear in the **Settings dialog** (⚙️ gear in nav footer). Users can also enter any IANA identifier via "Custom…" (shown inline within the same dialog).

| Value | i18n key | Label (en) | Offset |
|-------|----------|------------|--------|
| `UTC` | `common.utc` | UTC | UTC+0 |
| `America/New_York` | `common.eastern` | Eastern (ET) | UTC-5/-4 |
| `America/Chicago` | `common.central` | Central (CT) | UTC-6/-5 |
| `America/Denver` | `common.mountain` | Mountain (MT) | UTC-7/-6 |
| `America/Los_Angeles` | `common.pacific` | Pacific (PT) | UTC-8/-7 |
| `Europe/London` | `common.london` | London (GMT) | UTC+0/+1 |
| `Africa/Johannesburg` | `common.johannesburg` | South Africa (SAST) | UTC+2 |
| `Asia/Seoul` | `common.kst` | Korea (KST) | UTC+9 |
| `Asia/Tokyo` | `common.jst` | Japan (JST) | UTC+9 |
| `Australia/Sydney` | `common.sydney` | Sydney (AEST) | UTC+10/+11 |
| `Pacific/Auckland` | `common.auckland` | Auckland (NZST) | UTC+12/+13 |
| _(any IANA)_ | `common.custom` | Custom… | — |

**Custom timezone**: Selecting "Custom…" in the timezone Select reveals an inline IANA input within the Settings dialog (no nested modal). The user enters any IANA timezone string (e.g. `America/Sao_Paulo`), saves with Enter or the Save button. Validated via `isValidTimezone()` (Intl.DateTimeFormat). The `Timezone` type is `PresetTimezone | (string & {})` — accepts any valid IANA string. The cookie stores the raw IANA string; on reload, `readCookie()` validates with `isValidTimezone()` before accepting.

### Locale → Default Timezone

When `veronex-tz` cookie is absent, `TimezoneProvider` picks a default from locale:

| Locale | Default |
|--------|---------|
| `ko` | `Asia/Seoul` |
| `ja` | `Asia/Tokyo` |
| `en` (or any) | `America/New_York` |

Changing language in the nav calls `resetToLocaleDefault(locale)` → only takes effect if no explicit cookie. User-selected timezone is sticky.

### Date Formatter SSOT (`web/lib/date.ts`)

All date display goes through these functions — **never call `toLocaleString()` or `toLocaleDateString()` directly**:

```ts
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime, fmtDatetimeShort, fmtDateOnly, fmtHourLabel } from '@/lib/date'

// In component:
const { tz } = useTimezone()

fmtDatetime(iso, tz)        // "Mar 1, 12:34:56" — job detail, audit
fmtDatetimeShort(iso, tz)   // "Mar 1, 12:34"    — dashboard recent jobs
fmtDateOnly(iso, tz)        // "Mar 1, 2026"     — API keys, registered_at
fmtHourLabel(iso, tz)       // "3/1 14h"         — hourly chart x-axis
```

Backend always returns ISO 8601 UTC strings. All timezone conversion is client-side only.

### Backend UTC Guarantee

All PostgreSQL columns use `TIMESTAMPTZ` → stored and returned as UTC. sqlx deserializes to `DateTime<Utc>` → serialized to ISO 8601 with `Z` suffix. No timezone info on the server.

---

## Dashboard Page (`/overview`)

Integrated system health dashboard — answers "is the system healthy?" at a glance.
No tabs — renders `DashboardTab` directly.

### Component Files

| File | Role |
|------|------|
| `web/app/overview/page.tsx` | Data fetching (11 queries) + renders `DashboardTab` |
| `web/app/overview/components/dashboard-tab.tsx` | All 8 KPI/chart sections |

---

## Network Flow (Jobs Page Tab)

Real-time inference traffic visualization. Accessible as the **3rd tab** on `/jobs` page.
The standalone `/flow` route still exists but is **not linked in the nav**.

### Component Files

| File | Role |
|------|------|
| `web/app/jobs/page.tsx` | Jobs page — renders `<NetworkFlowTab>` as 3rd tab |
| `web/app/flow/page.tsx` | Standalone page (legacy nav route, still accessible directly) |
| `web/app/overview/components/network-flow-tab.tsx` | Composes ProviderFlowPanel + LiveFeed; accepts `backends` prop |
| `web/app/overview/components/provider-flow-panel.tsx` | SVG topology: Veronex API → Queue → Ollama / Gemini (4-column) |
| `web/app/overview/components/live-feed.tsx` | Scrollable real-time event list |
| `web/hooks/use-inference-stream.ts` | 5s TanStack Query polling of `GET /v1/dashboard/jobs?limit=50` |

---

### Dashboard Tab

All 8 sections are in `dashboard-tab.tsx`. `page.tsx` passes data as props.

#### Data Fetches (all parallel, graceful degradation)

| Query | Source | refetch | Notes |
|-------|--------|---------|-------|
| `api.stats()` | PostgreSQL | 30s | includes `test_keys` count |
| `api.backends()` | PostgreSQL | 30s | |
| `api.servers()` | PostgreSQL | 60s, retry:false | |
| `useQueries` per server → `api.serverMetrics(id)` | node-exporter | 30s, retry:false | live W, scrape_ok |
| `useQueries` per server → `api.serverMetricsHistory(id, 1440)` | ClickHouse | stale 5m, retry:false | 60-day kWh history |
| `api.performance(24)` | ClickHouse | 60s, retry:false | P50/P95/P99 + hourly |
| `api.performance(168)` | ClickHouse | 5min, retry:false | 7-day perf (`perf7d`) |
| `api.performance(720)` | ClickHouse | 10min, retry:false | 30-day perf (`perf30d`) |
| `api.usageAggregate(24)` | ClickHouse | 60s, retry:false | |
| `api.usageBreakdown(24)` | ClickHouse | 60s, retry:false | provider per model |
| `api.jobs('limit=10')` | PostgreSQL | 30s | |

ClickHouse-dependent values show `"—"` when ClickHouse is offline (graceful degradation).

#### Server Health (client-side derived)

```ts
const serverStatus = servers.map((s, i) => {
  const m = serverMetricQueries[i]?.data
  const connected = m?.scrape_ok === true
  const maxTemp = connected ? max(m.gpus[*].temp_c) : null
  const thermal = maxTemp == null ? 'unknown'
    : maxTemp >= 90 ? 'critical' : maxTemp >= 80 ? 'warning' : 'normal'
  return { id, name, connected, maxTemp, thermal }
})
```

`ThermalLevel`: `'normal' | 'warning' | 'critical' | 'unknown'`

- `connected` = `scrape_ok === true` from `NodeMetrics`
- `maxTemp` = `reduce(max, gpus[*].temp_c)` — works for 1-GPU APU (AMD AI 395+) and multi-GPU NVIDIA equally
- Server Health card always shows ALL registered servers (not just hot ones)
- Thermal Alert banner is separate, shown only when ≥1 server is ≥80°C
- **Status counts** (header row, shown only when servers > 0):
  - Connection: `connectedCount` (always shown) + `unreachableCount` (shown only when > 0)
  - Thermal: `normalCount` (always shown) + `warningCount` / `criticalCount` (shown only when > 0)
  - Derived from `serverStatus[]` in the component — no extra API call

#### Power Calculation (frontend-only)

```ts
// Actual kWh from history — hours=1440 returns 60-min buckets → 1 point = 1 kWh/kW
function sumKwhInRange(startMs: number, endMs: number): number
function sumKwhInWindow(fromHoursAgo: number, toHoursAgo: number): number
// fromHoursAgo MUST be > toHoursAgo (older bound first)

// History span — to show "N days of data" when accumulating
const historySpanD = (maxTs - minTs) / 86_400_000

// Daily Power: today midnight → now vs same weekday last week (full day)
const kwhToday     = sumKwhInRange(midnight, now)
const kwhSameDay7d = sumKwhInRange(midnight - 7d, midnight - 6d)
const dailyDelta   = kwhSameDay7d > 0 ? kwhToday - kwhSameDay7d : null

const kwhThisWeek  = sumKwhInWindow(168, 0)
const kwhLastWeek  = sumKwhInWindow(336, 168)
const weekDelta    = kwhLastWeek > 0 ? kwhThisWeek - kwhLastWeek : null
```

**Power history accumulation**: When `historySpanD < 7` (weekly) or `< 30` (monthly), show `t('overview.daysData', { n })` as subtitle instead of delta — explains "0.0 kWh" during early history collection. This is normal on new deployments.

#### Provider Taxonomy

Backends are grouped into two **generic categories** (future-proof):

| Category | i18n key | Icon | `backend_type` values | Examples |
|----------|----------|------|----------------------|---------|
| **Local** | `overview.localProviders` | `Server` | `['ollama']` | Ollama, vLLM, LocalAI |
| **API Services** | `overview.apiProviders` | `Globe` | `['gemini']` | Gemini, OpenAI, Anthropic |

**Rule**: Never hard-code "Ollama" or "Gemini" labels in Overview. Use `localProviders`/`apiProviders` i18n keys.

#### Thermal Alert (conditional banner — between Section 1 and Section 2)

Shown only when ≥1 registered GPU server reports GPU temp ≥ 80°C.

```
Thresholds:
  temp < 80°C  → hidden (no DOM element)
  80–89°C      → WARNING: amber border/bg (border-status-warning/40 bg-status-warning/5)
  ≥ 90°C       → CRITICAL: red border/bg (border-status-error/40 bg-status-error/5)
  Banner border switches to red if ANY server is critical.

Layout:
  [🌡 Thermal Alert — N server(s) need attention]      [Check Servers →]
  [🌡 gpu-node-1  95°C  Critical]  [🌡 gpu-node-2  83°C  Warning]
```

i18n keys: `overview.thermalAlert`, `overview.thermalAlertDesc`, `overview.tempCritical`, `overview.tempWarning`, `overview.checkServers`.

#### Layout (8 sections)

```
Section 1 — System KPIs (grid-cols-1 sm:grid-cols-3)
  [Provider Status N/M online] [Waiting — pending jobs] [Running — active jobs]

[Thermal Alert — conditional amber/red banner — only when ≥1 server ≥80°C]

Section 2 — Infrastructure (grid-cols-1 md:grid-cols-3)
  ┌──────────────────────────┐  ┌──────────────────────────────────────────────┐
  │ Server Health  (3)        │  │ Daily Power    Weekly Power    Monthly Power │
  │ 🟢 2 Connected  🔴 1 ✗   │  │ (StatsCard)    (StatsCard)     (StatsCard)   │
  │ ✓ 2 Normal  ⚠ 1 Warning  │  │ 0.02 kWh       0.1 kWh         0.3 kWh      │
  │ ─────────────────────    │  │ vs same day    N days data      N days data   │
  │ server-name               │  └──────────────────────────────────────────────┘
  │   Connected  Normal      │
  │ server-2                  │
  │   Unreachable  —         │
  └──────────────────────────┘
  Server Health card header: title + (total) + status count row (Connected/Unreachable + Normal/Warning/Critical)
  Per-server list below: name | ConnectionDot | ThermalBadge — unchanged
  Power cards show "N.N days of data" subtitle when historySpanD < 7 (weekly) or < 30 (monthly)

Section 3 — Workload + Latency Monitor (grid-cols-1 md:grid-cols-2)
  ┌─────────────────────────┐  ┌─────────────────────────┐
  │ Requests & Success      │  │ Latency                 │
  │          Daily  Weekly  Monthly │          Daily  Weekly  Monthly │
  │ Requests  1.2K   8.4K   32.1K  │ P50      450ms  480ms  510ms   │
  │ Success   99%    98%    97%    │ P95      1.2s   1.3s   1.4s    │
  │                                │ P99      2.1s   2.3s   2.5s    │
  └─────────────────────────┘  │ [mini sparkline — 24h avg/hr]  │
                                └─────────────────────────────────┘
  Row-based tables: metric rows × time-period columns (Daily/Weekly/Monthly)
  Success rate color: ≥99% green, ≥95% amber, <95% red
  Latency color: P50 warn≥1s/err≥3s; P95 warn≥2s/err≥5s; P99 warn≥5s/err≥10s

Section 4 — Provider Status + API Keys (grid-cols-1 md:grid-cols-2)

Section 5 — Request Trend (full-width AreaChart, hidden if empty)

Section 6 — Top Models (full-width horizontal BarChart, top 8)
  Bar: Cell per bar — Local=var(--theme-primary), API=var(--theme-status-info)

Section 7 — Recent Jobs (full-width mini table, 10 rows)

Section 8 — Token Summary (full-width)
```

#### Status Indicator Rules (2026 Best Practices)

Follow Carbon Design System 3-element rule: **color + icon + text** (never color alone — WCAG 1.4.1).

| Status | Component | Color class | Icon | Text |
|--------|-----------|-------------|------|------|
| Connected | `ConnectionDot` | `text-status-success-fg` | filled dot | `overview.connected` |
| Unreachable | `ConnectionDot` | `text-status-error-fg` | filled dot | `overview.unreachable` |
| Normal (<80°C) | `ThermalBadge` | `text-status-success-fg` | `CheckCircle2` | `overview.tempNormal` |
| Warning (80–89°C) | `ThermalBadge` | `text-status-warning-fg` | `AlertTriangle` | `overview.tempWarning` |
| Critical (≥90°C) | `ThermalBadge` | `text-status-error-fg` | `XCircle` | `overview.tempCritical` |

i18n keys: `overview.serverHealth`, `overview.connected`, `overview.unreachable`, `overview.tempNormal`, `overview.tempWarning`, `overview.tempCritical`, `overview.noServers`, `overview.daysData`.

---

### Network Flow Page (`/flow`) — Detail

Real-time **3-phase bidirectional** visualization of the inference pipeline — **3-column topology** (GPU server column removed).
Data source: `useInferenceStream` SSE hook — connects to `GET /v1/dashboard/jobs/stream`.
Page fetches only `backends` (no `servers`, no heavy analytics queries).

#### Layout

```
┌────────────────────────────────────────────────────────────────┐
│  Provider Flow                                                  │
│                                                                 │
│  [Veronex API] ──🟡──→ [Queue] ──🔵──→ [Ollama]              │
│                         ·····←🟢·····                          │
│                                 ──🔵──→ [Gemini]               │
│                         ············←🟢·····                   │
│                                                                 │
│  N req/5m below each provider node                              │
└────────────────────────────────────────────────────────────────┘
┌────────────────────────────────────────────────────────────────┐
│  Live Feed  ● live                                              │
│  ● llama3   Ollama   completed  423ms   just now               │
│  ● qwen3:8b Ollama   running    —       5s ago                 │
│  ● gemini   Gemini   pending    —       10s ago                │
└────────────────────────────────────────────────────────────────┘
```
Status updates live: pending → running → completed/failed/cancelled (latencyMs filled on completion)

Legend: 🟡 enqueue (amber) · 🔵 dispatch (blue) · 🟢 response (green/red, dimmed) · ····· bypass arc

#### Bee phases and routing

| Phase | Direction | Trigger | Status | Visual |
|-------|-----------|---------|--------|--------|
| `enqueue` | API → Queue | `pending` SSE event | `pending` | Full opacity, yellow (`#facc15`) |
| `dispatch` | Queue → Provider | `running` SSE event | `running` | Full opacity, blue |
| `response` | Provider → API | `completed\|failed\|cancelled` SSE event | terminal | Dimmed (`cc` bg, `28` glow) |

Response arcs **bypass the Queue** — inferences don't re-queue on return.

**Ollama**: Queue→Ollama (dispatch) · Ollama→API arc above Queue (response)
**Gemini**: Queue→Gemini (dispatch) · Gemini→API arc below Queue (response)

#### useInferenceStream (`web/hooks/use-inference-stream.ts`)

SSE stream — `fetch()` to `GET /v1/dashboard/jobs/stream` with `Authorization: Bearer <JWT>`.

```ts
export interface FlowEvent {
  id: string           // jobId + status + spawn timestamp (unique)
  jobId: string
  provider: 'ollama' | 'gemini' | string   // looked up from backends[] by name
  backendName: string
  model: string
  status: string
  latencyMs: number | null
  ts: number           // ms when received
  phase: 'enqueue' | 'dispatch' | 'response'
}
```

**Phase mapping** (one-to-one from SSE event status — no state diffing needed):
- `pending` → `enqueue`
- `running` → `dispatch`
- `completed | failed | cancelled` → `response`

**Provider resolution**: `backendTypeMapRef.get(event.backend) ?? 'ollama'`
(looks up backend name → type from `backends[]` prop via `useRef`).

**Reconnection**: exponential backoff, `2s → 30s` max. No polling interval.
Rolling list capped at **50 events** (newest first).

#### Bee Particle Animation

**Engine**: CSS Motion Path (`offset-path: path(...)`) + `@keyframes bee-fly` in `globals.css`.
Not SVG SMIL — CSS is GPU-composited, SMIL is not (2026 best practice).

```css
/* globals.css */
@keyframes bee-fly {
  0%   { offset-distance: 0%;   opacity: 0; }
  6%   {                        opacity: 1; }
  88%  {                        opacity: 1; }
  100% { offset-distance: 100%; opacity: 0; }
}

.bee-particle {
  position: absolute;
  width: 10px; height: 10px; border-radius: 50%;
  will-change: offset-distance, opacity;   /* GPU layer promotion */
  animation: bee-fly 1400ms linear forwards;
  offset-anchor: 50% 50%;
}
```

**Coordinate system**: Fixed `360 × 240` logical space.
A `ResizeObserver` measures the container width and applies `transform: scale(w/360)`
to the bee overlay div, so `offset-path` coordinates always match `viewBox="0 0 360 240"`.

**State**: `useReducer(beeReducer, [])` — `SPAWN` adds bees, `EXPIRE` removes them.
Cleanup: `onAnimationEnd` → `EXPIRE` dispatch — no `setTimeout` leaks.
Max concurrent bees: **30** (`.slice(-MAX_BEES)` in reducer).

**Sequential hop stagger**: `BEE_STAGGER_MS = 700` (half of 1400 ms duration).
Hop 2 of each leg sets `animationDelay: 700ms` so bees appear to flow segment-by-segment.

**Bee color**:
- `enqueue` phase: always `#facc15` (yellow-400) — hardcoded `ENQUEUE_COLOR` constant
- Other phases: `statusColor(e.status)`:

| Status | Color |
|--------|-------|
| `pending` | `var(--theme-status-warning)` — amber |
| `running` | `var(--theme-status-info)` — blue (dispatch bees) |
| `completed` | `var(--theme-status-success)` — green (response bees) |
| `failed` | `var(--theme-status-error)` — red (response bees) |
| `cancelled` | `var(--theme-status-cancelled)` — slate/gray (response bees) |

Response bees: `backgroundColor: color + 'cc'`, `boxShadow: color + '28'` (dimmed).
`statusDotColor()` in `live-feed.tsx` follows the same mapping.

#### Provider Flow Panel — SVG Topology (540 × 264) — ArgoCD style

3-column layout, **ArgoCD-style distinct node shapes**. Max-width cap: 680 px (prevents unbounded growth on wide screens).
Response arcs from Provider bypass Queue (arc above/below Queue node).

```
Column    Node           Shape      cx    cy    dim                 stroke
───────────────────────────────────────────────────────────────────────────────
Col 1     Veronex API    Rect+accent  72   132   w=108 h=56          var(--theme-primary) + left accent bar (5px, clipped)
Col 2     Queue(Valkey)  Cylinder   244   132   rx=44 ry=10 body=44  var(--theme-border) — top ellipse fill=bg-elevated
Col 3     Ollama         Octagon    460    72   w=108 h=52 inset=10  providerStroke(localBs)
Col 3     Gemini         Octagon    460   192   w=108 h=52 inset=10  providerStroke(apiBs)

providerStroke: online→success | degraded→warning | offline→error | empty→border

Bee paths — enqueue:
  API → Queue:    M 126,132 C 150,132 176,132 200,132

Bee paths — dispatch:
  Queue → Ollama: M 288,132 C 318,132 376,72  406,72
  Queue → Gemini: M 288,132 C 318,132 376,192 406,192

Bee paths — response bypass:
  Ollama → API:   M 406,72  C 346,18  186,18  126,132
  Gemini → API:   M 406,192 C 346,246 186,246 126,132

Connection lines:
  Dispatch paths:  strokeDasharray='6 4', strokeWidth=1.5, markerEnd arrowhead
  Response bypass: strokeDasharray='3 7', strokeWidth=1, opacity=0.4 (faint arc)
```

Queue depth badge: shown below Queue cylinder bottom cap when `queueDepth > 0`.
- Poll: `GET /v1/dashboard/queue/depth` every 3 s (`queueDepthQuery`)
- Response: `{ api_paid, api, test, total }` — panel receives `total` as `queueDepth` prop
- Badge: `overview.queueWaiting` i18n key (`{{count}} waiting`)
- Valkey LLEN on: `veronex:queue:jobs:paid` + `veronex:queue:jobs` + `veronex:queue:jobs:test`

#### Live Feed

Scrollable table (`max-h-64`), newest event first.
Shows **`enqueue`-phase events only** (job arrivals) — dispatch/response are animation-only.
Columns: status dot · model (mono) · provider · status · latency · time ago.
Empty state: `overview.waitingRequests`.
Header pulse dot when events exist: `● live`.

**Live status updates**: displayed status is NOT frozen at 'pending' (enqueue time).
A `latestByJob` map is computed from ALL events (newest-first → first match per jobId = latest state).
- On `dispatch` SSE: row updates to `running` (blue dot)
- On `response` SSE: row updates to `completed`/`failed`/`cancelled` + latencyMs populated
- Source: `useMemo` over `events` prop (all phases); O(n) scan, instant on SSE event

#### i18n Keys (overview.* — Network Flow Page)

| Key | en |
|-----|----|
| `networkFlow` | Network Flow |
| `networkFlowDesc` | Real-time inference traffic visualization |
| `providerFlow` | Provider Flow |
| `liveFeed` | Live Feed |
| `waitingRequests` | Waiting for requests... |
| `queueWaiting` | {{count}} waiting |
| `reqLast5m` | `{{count}}` req / 5m |

---

### All i18n Keys (overview.*)

Base: `providerStatus`, `queueDepth`, `recentJobs`, `viewAllJobs`,
`tokenSummary`, `perfSummary`, `goToProviders`, `goToKeys`, `goToUsage`, `goToPerformance`

Infrastructure / provider: `infrastructure`, `gpuPower`, `gpuServers`, `weeklyPower`, `monthlyPower`,
`prevWeek`, `prevMonth`, `topModels`, `noServerPower`, `localProviders`, `apiProviders`,
`testKeys`, `activeKeysLabel`

Network flow: `networkFlow`, `networkFlowDesc`, `providerFlow`, `liveFeed`, `waitingRequests`, `reqLast5m`

### i18n Keys (nav.*)

`overview` (unused label), `monitor`, `dashboard`, `flow`, `jobs`, `keys`, `usage`, `performance`,
`servers`, `providers`, `ollama`, `gemini`, `accounts`, `audit`, `apiDocs`

### i18n Keys (common.* — settings dialog)

**Timezone**: `timezone`, `utc`, `eastern`, `central`, `mountain`, `pacific`, `london`, `johannesburg`, `kst`, `jst`, `sydney`, `auckland`, `custom`, `customTimezone`, `customTimezonePlaceholder`, `customTimezoneHint`, `customTimezoneInvalid`

**Settings / language**: `settings`, `language`

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
- Source: global `gemini_models` pool + `provider_selected_models` overrides

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
4. Extend `ProviderType` enum in Rust + add adapter in `infrastructure/outbound/`
5. Update `docs/llm/backend/backends-ollama.md` + `docs/llm/backend/openai.md`
6. Create `docs/llm/frontend/web-providers.md` section for the new tab
