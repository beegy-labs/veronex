# Web -- Brand, Design System & Core

> SSOT | **Last Updated**: 2026-03-25 | Split from monolithic design-system into 3 files

Related files:
- [design-system-i18n.md](design-system-i18n.md) -- i18n, timezone, date formatting
- [design-system-components.md](design-system-components.md) -- auth guard, login, API client, status colors, flow viz, adding provider

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new data table | `web/components/data-table.tsx` (SSOT) | Use `<DataTable minWidth="...">` -- never write raw Card/Table boilerplate |
| Add new nav link | `web/components/nav.tsx` `navItems` + `web/messages/en.json` `nav.*` | Add item + i18n key in all 3 locales |
| Add new color token | `web/app/tokens.css` | Layer 1 (`--palette-*`) -> Layer 2 (`--theme-*`) -> Layer 0 (`@property`) -> Layer 3 (`@theme inline`) |
| Add new locale | See [design-system-i18n.md](design-system-i18n.md) | i18n config + message file + timezone default |
| Add new provider type | See [design-system-components.md](design-system-components.md) | 5-step process |
| Add public (no-auth) route | See [design-system-components.md](design-system-components.md) | `PUBLIC_PATHS` array |
| Change theme colors | `web/app/tokens.css` Layer 2 `--theme-*` | Only edit `--theme-*`, never hardcode hex in TSX |
| Add flow visualization panel | `web/app/overview/components/` | See [design-system-components.md](design-system-components.md) |
| Display a new date/time field | See [design-system-i18n.md](design-system-i18n.md) | `fmtDatetime`/`fmtDatetimeShort`/`fmtDateOnly` |
| Gate component on lab feature | `web/components/lab-settings-provider.tsx` | `const { labSettings } = useLabSettings()` |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/tokens.css` | Design token SSOT (4-layer architecture) |
| `web/lib/design-tokens.ts` | TypeScript token module — type-safe `tokens.*` references for inline styles |
| `web/lib/constants.ts` | Tailwind badge/color class maps (PROVIDER_BADGE, STATUS_STYLES, etc.) |
| `web/lib/chart-theme.ts` | Recharts style constants + formatters — uses `tokens.*` internally |
| `web/app/globals.css` | Tailwind v4 entry + focus ring + bee animation |
| `web/app/layout.tsx` | All providers: Theme, I18n, Timezone, QueryClient, LabSettings |
| `web/components/lab-settings-provider.tsx` | `useLabSettings()` -- experimental feature flags |
| `web/components/nav.tsx` | Collapsible sidebar (imports `HexLogo` from `nav-icons.tsx`) |
| `web/components/nav-icons.tsx` | `HexLogo` + `OllamaIcon` SVGs |
| `web/components/nav-settings-dialog.tsx` | Settings dialog: language, timezone, lab features |
| `web/components/theme-provider.tsx` | `data-theme` switcher, `localStorage('hg-theme')` |
| `web/components/data-table.tsx` | `DataTable` + `DataTableEmpty` -- SSOT for all tables |
| `web/lib/auth.ts` | Token CRUD (see [components](design-system-components.md)) |
| `web/lib/auth-guard.ts` | Auth flow SSOT (see [components](design-system-components.md)) |
| `web/lib/api-client.ts` | HTTP transport, delegates 401 to auth-guard |
| `web/lib/api.ts` | All API call functions |
| `web/lib/types.ts` | All TypeScript types |

---

## Brand (Veronex)

- **Name**: Vero (truth/precision) + Nexus (connection hub)
- **Logo**: `HexLogo` in `nav-icons.tsx` -- flat-top honeycomb hexagon SVG, 32x32 viewBox
- **Logo CSS vars**: `var(--theme-logo-start)`, `var(--theme-logo-end)`
- **Favicon**: `web/public/favicon.svg` -- forest gradient `#0d2518 -> #16402e`
- **Wordmark**: `web/public/logo.svg` -- hex mark + "Veronex" text in `#16402e`
- **Dark mode logo**: violet gradient `#a78bfa -> #c4b5fd`

---

## Design Theme -- "Verde Nexus"

| Attribute | Light "Platinum Signal" | Dark "Obsidian Verde" |
|---|---|---|
| Page bg | `#f2f4f2` Platinum Pearl | `#080a09` Obsidian Deep |
| Card bg | `#ffffff` Pure White | `#111412` Dark Graphite |
| Primary | `#0f3325` Deep Ivy (12.71:1 AAA) | `#10b981` Bio-Emerald (7.73:1 AAA) |
| Text primary | `#141a14` Anthracite ~14.4:1 AAA | `#e2e8e2` Soft Platinum ~14.2:1 AAA |
| Text secondary | `#334155` Slate Silver ~10:1 AAA | `#94a3b8` Titanium Silver ~7.7:1 AAA |
| Border | `#e2e8e0` | `#1a2118` |
| Button fg | `#ffffff` on Deep Ivy | `#041f16` Deep Dark on Bio-Emerald |

WCAG targets: Primary >=7:1 (AAA), body text AAA, status colors AAA both modes.
Light logo: `#091e12 -> #0f3325`. Dark logo: `#047857 -> #10b981` (emerald gradient).
Dark status colors: `#34d399` / `#fb7185` / `#fbbf24` / `#60a5fa`.

---

## tokens.css -- 4-Layer Token Architecture

```css
/* Layer 0: @property -- type safety + CSS transition support */
@property --theme-primary { syntax: '<color>'; ... }

/* Layer 1: --palette-* raw hex (NEVER use in components) */
--light-primary: #16402e;

/* Layer 2: --theme-* semantic (switches via [data-theme='dark']) */
--theme-primary: var(--light-primary);
[data-theme='dark'] --theme-primary: ...;

/* Layer 3: @theme inline -- Tailwind utility generation */
@theme inline { --color-primary: var(--theme-primary); }
```

**Token flow for new tokens**: Layer 1 -> Layer 2 -> Layer 0 -> Layer 3.

---

## Key Policies

| Policy | Rule |
|--------|------|
| Color — inline style | Use `tokens.*` from `web/lib/design-tokens.ts` — never raw `'var(--theme-*)'` strings |
| Color — Tailwind class | Use semantic utilities: `bg-status-success`, `text-status-warning-fg` — never `gray-*`/`slate-*`/`zinc-*` |
| Color — hardcoded hex | Zero tolerance in TSX. Exception: `redoc-wrapper.tsx` (3rd-party theme API, documented inline) |
| Token names | `status-warning` / `status-warning-fg` — NOT `status-warn` / `status-warn-fg` |
| SVG / Recharts | `fill={tokens.*}` (JSX expression) — never `fill="var(--theme-*)"` string attribute |
| Headings | `text-2xl font-bold tracking-tight` |
| Status order | Always: pending → running → completed → failed → cancelled |
| i18n | All user-visible strings via `t('key')` — no hardcoded English/Korean/Japanese |
| i18n interpolation | Always use `{{var}}` double braces for interpolation — never single `{var}` |
| i18n parity | Every key in `en.json` must exist in `ko.json` and `ja.json` |
| CJK overflow | Use `whitespace-nowrap` on badges and table headers to prevent CJK text wrapping mid-word |
| Terminology | See `docs/llm/policies/terminology.md` — SSOT for all term definitions |
| Recharts style | Import from `web/lib/chart-theme.ts` (SSOT) — never define chart constants in page files |
| Recharts formatters | Use `fmtMs`, `fmtCompact`, `fmtPct`, `fmtTemp` etc from `chart-theme.ts` — no local `toFixed()` for display |
| Accessibility | WCAG 2.1 AA (admin scope): color+icon+text for status, `aria-label` on icon-only buttons, focus ring |
| Focus ring | `4px solid var(--theme-focus-ring)`, offset 4px |
| Font | System font stack only — no Google Fonts (breaks CJK) |

---

## Nav Sidebar (nav.tsx)

```
[Monitor]           <- collapsible group (default OPEN)
  Dashboard         -> /overview
  Usage             -> /usage
  Performance       -> /performance
Jobs                -> /jobs             <- standalone link; 3 tabs
API Keys            -> /keys
Servers             -> /servers          <- standalone link
[Providers]         <- collapsible group
  Ollama            -> /providers?s=ollama
  Gemini            -> /providers?s=gemini

Footer:
  API Docs          -> /api-docs
  [Accounts]        -> /accounts         <- JWT only
  [Audit Log]       -> /audit            <- JWT + super role only
  username / logout
  v0.1.0 / [Settings gear] / [theme toggle]
```

**Settings gear**: opens `NavSettingsDialog` (`nav-settings-dialog.tsx`) with Language row (en/ko/ja) + Timezone row (11 presets + "Custom...") + Lab features toggle.

| Property | Value |
|----------|-------|
| Width | `w-56` expanded / `w-14` collapsed; `transition-all duration-200` |
| Collapse state | `localStorage('nav-collapsed')` |
| Group state | `localStorage('nav-group-{id}')`, default open for `id: 'overview'` |
| Active detection | `isChildActive()` per child, not `basePath.startsWith` |
| Mobile | hamburger slide sidebar, `w-72`, backdrop close, auto-close on route change |

---

## DataTable Component (SSOT)

All data tables use `<DataTable>` from `web/components/data-table.tsx`.

```tsx
<DataTable minWidth="700px">
  <TableHeader>...</TableHeader>
  <TableBody>...</TableBody>
</DataTable>

<DataTable minWidth="700px" footer={totalPages > 1 ? <PaginationRow /> : undefined}>
  ...
</DataTable>
```

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `minWidth` | `string` | `'600px'` | Minimum table width before horizontal scroll |
| `footer` | `ReactNode` | -- | Optional footer (e.g. pagination) |

Base padding: `TableHead` `h-11 px-4`, `TableCell` `py-3 px-4`. First cell `pl-6`, last `pr-6`. Never override edge padding.

---

## State Management

- Server state: TanStack Query (`useQuery`, `useMutation`); local state: `useState` for modals
- No global client store (no Redux/Zustand)
- QueryClient config (`layout.tsx`): `staleTime: 30_000`, `retry: 1`, `refetchOnWindowFocus: false`
- `refetchOnWindowFocus: false` prevents burst refetch on tab re-focus and avoids racing the token refresh mutex

---

## Next.js 16.2 / React 19.2 — Key Patterns

### `<Activity>` — State-Preserving Hide/Show

Replaces `{condition && <Component />}` when the component must retain state across hide/show:

```tsx
import { Activity } from 'react'

// tab panels, collapsible sections, back-navigation preserved state
<Activity mode={isVisible ? 'visible' : 'hidden'}>
  <ExpensivePanel />
</Activity>
```

When hidden: effects unmount, updates are deferred. State survives. Use instead of conditional render when remounting is expensive or state loss is unacceptable.

### `unstable_retry()` in error.tsx

Prefer `unstable_retry()` over `reset()` for data-fetching errors — it does `router.refresh()` + `reset()` inside a transition:

```tsx
// app/*/error.tsx
export default function Error({ reset, retry }: { reset: () => void; retry: () => void }) {
  return <Button onClick={retry}>{t('common.retry')}</Button>
}
```

### `useId` Prefix (React 19.2)

`useId()` now emits IDs with prefix `_r_` (was `:r:` in 19.0). Update any snapshot tests or DOM assertions that check `useId` output. The current pattern in `nav-progress.tsx` (stripping `:`) should be updated to strip `_` instead:

```tsx
const safeId = rawId.replace(/_/g, '') // React 19.2: IDs are "_r0_" format
```

### Next.js 16.2 — No mandatory code changes

The 16.1.6 → 16.2.1 upgrade is a safe bump. New opt-in flags (all off by default):
- `experimental.prefetchInlining` — reduces prefetch requests per link
- `experimental.appNewScrollHandler` — improved post-navigation focus management

RSC payload deserialization is ~350% faster in 16.2 (zero config gain).

---

## Duration / Latency Formatter -- `fmtMs` (SSOT)

Shared in `web/lib/chart-theme.ts`. Use everywhere -- never inline ms conversion.

| Function | Use case | Example |
|----------|----------|---------|
| `fmtMs(n)` | KPI cards, tooltips, table cells | `86360` -> `"1m 26s"` |
| `fmtMsAxis(n)` | Chart Y-axis tick labels | `86360` -> `"1.4m"` |
| `fmtMsNullable(n)` | Nullable job latency | `null` -> `"--"` |

Tiers: `< 1s` -> `"Xms"` / `1s-59s` -> `"X.Xs"` / `1m-59m` -> `"Xm Xs"` / `>= 1h` -> `"Xh Xm"`.
