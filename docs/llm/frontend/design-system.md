# Web -- Brand, Design System & Core

> SSOT | **Last Updated**: 2026-05-03

> **Stack (post-migration, 2026-05-03)**: verodesign `vds-*` utilities. Tailwind has been fully removed.
> No `tailwindcss`, `tailwind-merge`, `@radix-ui/*`, `class-variance-authority` deps. Class concat goes
> through `lib/vds-merge.ts` (last-wins per CSS-property family). The entire pipeline is verified by
> `pnpm lint:no-tailwind` (0 hits across 163 files) and `pnpm verify:contrast` (22/22 WCAG pairs pass).

Related files:
- [design-system-i18n.md](design-system-i18n.md) -- i18n, timezone, date formatting
- [design-system-components.md](design-system-components.md) -- auth guard, login, API client, status colors, flow viz, adding provider
- [design-system-components-patterns.md](design-system-components-patterns.md) -- provider taxonomy, network flow viz, accounts, dialogs
- [design-system-patterns.md](design-system-patterns.md) -- Next.js/React 19 patterns, fmtMs formatter

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new data table | `web/components/data-table.tsx` (SSOT) | Use `<DataTable minWidth="...">` -- never write raw Card/Table boilerplate |
| Add new nav link | `web/components/nav.tsx` `navItems` + `web/messages/en.json` `nav.*` | Add item + i18n key in all 3 locales |
| Add new color token | `web/app/styles/vds/theme-veronex.css` | Add `--vds-theme-<name>: light-dark(<light>, <dark>)` inside the `[data-theme="veronex"], [data-theme="veronex"] *` block. Mirror the entry in `web/lib/design-tokens.ts` for type-safe TSX access |
| Add new locale | See [design-system-i18n.md](design-system-i18n.md) | i18n config + message file + timezone default |
| Add new provider type | See [design-system-components.md](design-system-components.md) | 5-step process |
| Add public (no-auth) route | See [design-system-components.md](design-system-components.md) | `PUBLIC_PATHS` array |
| Change theme colors | `web/app/styles/vds/theme-veronex.css` `--vds-theme-*` | Edit token only; never hardcode hex in TSX. Run `pnpm verify:contrast` after any change |
| Add flow visualization panel | `web/app/overview/components/` | See [design-system-components.md](design-system-components.md) |
| Display a new date/time field | See [design-system-i18n.md](design-system-i18n.md) | `fmtDatetime`/`fmtDatetimeShort`/`fmtDateOnly` |
| Gate component on lab feature | `web/components/lab-settings-provider.tsx` | `const { labSettings } = useLabSettings()` |
| Use Next.js Activity / unstable_retry | See [design-system-patterns.md](design-system-patterns.md) | State-preserving hide/show, error retry |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/styles/vds/theme-veronex.css` | Verde Nexus token SSOT — overrides verodesign defaults via `[data-theme="veronex"], [data-theme="veronex"] *` selector (descendant duplication is required because verodesign's `@property --vds-theme-*` declarations use `inherits: false` — see "Cascade gotcha" below) |
| `web/app/styles/vds/full.css` | verodesign utility bundle (copied from `verodesign/packages/design/dist/css/`). 14k+ lines, do NOT edit |
| `web/app/styles/vds/{state-variants,responsive,animations}.css` | verodesign sub-bundles for hover/focus/sm/md/lg utilities + keyframes |
| `web/app/globals.css` | App-only overrides layer: typography floor, focus ring, `bee-particle` animation, **arbitrary-value `vds-*-[…]` shims** (verodesign does not emit `vds-h-[60px]` etc., so each arbitrary value used in code is hand-listed here) |
| `web/lib/vds-merge.ts` | `cn()` last-wins class merger (replaces `tailwind-merge`). Conflict-keys understand size vs. color disambiguation for `border`/`divide`/`ring`/`outline` |
| `web/lib/design-tokens.ts` | TypeScript token module — type-safe `tokens.*` references for inline styles + SVG fills |
| `web/lib/constants.ts` | Verodesign badge/status class maps (PROVIDER_BADGE, STATUS_STYLES, etc.) |
| `web/lib/chart-theme.ts` | Recharts style constants + formatters — uses `tokens.*` internally |
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
- **Logo CSS vars**: `var(--vds-theme-logo-start)`, `var(--vds-theme-logo-end)`, `var(--vds-theme-logo-inner)`
- **Logo gradient ID**: must be per-instance (use `useId()`). Two HexLogos co-exist (mobile top bar + desktop sidebar) and a hard-coded `id="hex-grad"` collides — `fill="url(#…)"` resolves to the first hidden match and the visible logo paints empty.
- **Favicon**: `web/public/favicon.svg` -- forest gradient `#0d2518 -> #16402e`
- **Wordmark**: `web/public/logo.svg` -- hex mark + "Veronex" text in `#16402e`

---

## Design Theme -- "Verde Nexus" (current values, theme-veronex.css v6 / 2026-05-03)

The dark-mode ramp went through six iterations. Current values are tuned for **eye comfort + premium feel** — pure-black halation avoidance (Bartlett 2017), readable dim text, and a slight Verde-hue chroma so surfaces don't read "flat gray".

| Attribute | Light "Platinum Signal" | Dark "Obsidian Verde" | Token |
|---|---|---|---|
| Page bg | `oklch(96% 0.004 150)` ≈ `#f0f2f0` | `#080a09` Obsidian Deep | `--vds-theme-bg-page` |
| Card bg | `oklch(100% 0 0)` `#fff` | `#101412` Dark Graphite (hsl 150,11,7 — user-pinned) | `--vds-theme-bg-card` |
| Elevated | `#fff` | `#161a18` | `--vds-theme-bg-elevated` |
| Hover | `oklch(93% 0.008 150)` | `#1a1e1c` | `--vds-theme-bg-hover` |
| Muted | `oklch(89% 0.01 150)` | `#1f2321` | `--vds-theme-bg-muted` |
| Primary | `oklch(30% 0.06 150)` Deep Ivy (11.85:1 AAA) | `oklch(72% 0.16 165)` Bio-Emerald (8.68:1 AAA) | `--vds-theme-primary` |
| Text primary | `oklch(15% 0.012 240)` (17.5:1) | `oklch(96% 0.005 150)` Soft Platinum (17.7:1) | `--vds-theme-text-primary` |
| Text secondary | `oklch(32% 0.018 240)` | `oklch(86% 0.006 150)` (9.3:1) | `--vds-theme-text-secondary` |
| Text dim | `oklch(48% 0.02 240)` | `oklch(74% 0.007 150)` (8.1:1) | `--vds-theme-text-dim` |
| Text faint | `oklch(65% 0.015 240)` | `oklch(60% 0.008 150)` | `--vds-theme-text-faint` |
| Border subtle | `oklch(88% 0.008 150)` | `#1c2120` | `--vds-theme-border-subtle` |
| Border default | `oklch(76% 0.012 150)` | `#2a302d` | `--vds-theme-border-default` |
| Border focus | `oklch(40% 0.1 150)` | `oklch(72% 0.16 165)` | `--vds-theme-border-focus` |

**Status colors are mode-aware** (the previous single-OKLCH variant could not satisfy WCAG AA on both light card L100 and dark card L18 simultaneously):
- success: `light-dark(oklch(45% 0.171 145), oklch(74% 0.18 145))`
- warning: `light-dark(oklch(52% 0.18 70),  oklch(78% 0.175 70))`
- error: `light-dark(oklch(48% 0.2 25),   oklch(72% 0.2 25))`

WCAG targets enforced by `pnpm verify:contrast`: primary on bg-page AAA (≥7:1), body text AAA, dim/faint text AA-large (≥3:1), button fg AA (≥4.5:1), focus indicator UI (≥3:1). All 22 pairs pass in both modes as of 2026-05-03.

---

## verodesign Token Architecture

```css
/* verodesign emits @property declarations on every --vds-theme-* token */
@property --vds-theme-bg-card {
  syntax: '<color>';
  inherits: false;     /* ⚠ critical — see Cascade gotcha */
  initial-value: oklch(100% 0 0);
}

/* verodesign default theme-veronex (dist/css/themes/veronex.css)
   sits in @layer vds-tokens, declared on :root only */
@layer vds-tokens {
  :root[data-theme="veronex"] {
    --vds-theme-bg-card: light-dark(<light>, <dark>);
    /* ...50+ tokens... */
  }
}

/* lightningcss compiles `light-dark(A, B)` to:
     var(--lightningcss-light, A) var(--lightningcss-dark, B)
   The control vars are toggled by [data-mode] rules in full.css. */
```

**Cascade gotcha — descendant override required.** verodesign's `@property` declarations use `inherits: false`. That means descendants of `<html>` do NOT inherit token values — they fall back to `initial-value` (the verodesign default light palette). Toggling `data-mode` on `<html>` correctly flips the var on `:root`, but `<body>`, cards, buttons etc. keep showing the static initial value, so dark mode never visually applies and brand overrides on `:root` never reach descendants either.

**Fix (in `theme-veronex.css`)**: declare the override on `[data-theme="veronex"], [data-theme="veronex"] *`. Each descendant re-declares the token; `light-dark()` resolves via the inherited `color-scheme` + control vars. This is verified by Playwright probes after every theme edit.

Token flow when adding a new token:
1. Append to `[data-theme="veronex"], [data-theme="veronex"] *` block in `app/styles/vds/theme-veronex.css`
2. Mirror in `lib/design-tokens.ts` so TSX/SVG fills can use `tokens.<group>.<name>` (string `'var(--vds-theme-<name>)'`)
3. Run `pnpm verify:contrast` (extend `PAIRS` in `scripts/verify-contrast.mjs` if the new token is a text/bg pair)

---

## Migration audit (2026-05-03)

| Check | Status | Evidence |
|---|---|---|
| No `tailwindcss` / `tailwind-merge` / `@radix-ui` / `class-variance-authority` deps | ✅ | `package.json` design deps = `clsx` only |
| No `tailwind.config.*` / `postcss.config.*` for utility generation | ✅ | none exist |
| No bare-Tailwind class strings in `.tsx` (`bg-background`, `text-foreground`, `hover:bg-accent`, etc.) | ✅ | `pnpm lint:no-tailwind` — 0 hits across 163 files |
| All UI primitives (Button/Card/Input/Label/Select/Switch/Tabs/Dialog/Tooltip/Table/Badge/Checkbox/Separator) use `vds-*` only | ✅ | 13/13 components in `components/ui/` |
| Class merger replaces `tailwind-merge` | ✅ | `lib/vds-merge.ts` (size vs. color disambiguation for border/divide/ring/outline) |
| Verde Nexus theme overrides cascade to all descendants | ✅ | `[data-theme="veronex"] *` selector — Playwright cycle test (light/dark/light/dark) |
| WCAG contrast 22/22 pairs pass | ✅ | `pnpm verify:contrast` — primary AAA, status AA, focus UI |
| Day/night toggle works at all DOM levels | ✅ | `data-mode` switches both `:root` control vars and inherited descendant tokens |
| HexLogo paints in both desktop sidebar + mobile top bar | ✅ | per-instance `useId()` for gradient ID (resolves duplicate-ID collision) |
| Arbitrary-value classes (`vds-h-[60px]`, `vds-sm:w-[560px]`, `vds-text-[11px]`, …) | ✅ | hand-listed in `app/globals.css` overrides layer (verodesign does not emit per-pixel arbitrary utilities — keep the list IN SYNC with `grep -rEho 'vds-[a-z-]+\[[^\]]+\]' app components`) |

Open follow-ups: none. Future verodesign upstream changes that touch `--vds-theme-*` schema must be mirrored in `theme-veronex.css` (the file is a copy with the descendant-cascade selector fix applied).

---

## Key Policies

| Policy | Rule |
|--------|------|
| Color — single source | All colors live in `web/app/styles/vds/theme-veronex.css`. Change = edit `--vds-theme-*` only. Touching `.tsx` to change a color = policy violation |
| Color — inline style | Use `tokens.*` from `web/lib/design-tokens.ts` (returns `'var(--vds-theme-*)'`). Never embed raw token strings inline |
| Color — utility class | Use verodesign semantic `vds-*`: `vds-bg-success`, `vds-text-warning`, `vds-border-default`, etc. Never `vds-gray-*` raw scale or `vds-bg-[#123]` arbitrary hex |
| Color — hardcoded hex | Zero tolerance in TSX. Exception: `redoc-wrapper.tsx` (3rd-party theme API — Redoc requires static hex via JS object, does not support CSS variables) |
| Arbitrary-value utilities | verodesign does NOT emit per-pixel `vds-h-[60px]` / `vds-sm:w-[560px]` / `vds-text-[11px]` etc. New arbitrary values must be appended to the explicit-list block in `app/globals.css` (overrides layer). Prefer the standard scale (`vds-h-15`, `vds-max-w-xl`) when possible |
| 3rd-party CSS overrides | Put in a dedicated `.css` file and `import` it. Never use `<style>{`...`}</style>` inline blocks in `.tsx` |
| Dark mode selector | `[data-theme="veronex"][data-mode="dark"]` (verodesign convention). Toggle via `data-mode` on `<html>`; FOUC-safe init script runs in `<head>` before hydration |
| Token names | `status-warning` / `status-warning-fg` — NOT `status-warn` |
| SVG / Recharts | `fill={tokens.*}` (JSX expression) — never `fill="var(--theme-*)"` string attribute |
| Headings | `text-2xl font-bold tracking-tight` |
| Status order | Always: pending → running → completed → failed → cancelled |
| i18n | All user-visible strings via `t('key')` — no hardcoded English/Korean/Japanese |
| i18n interpolation | Always use `{{var}}` double braces — never single `{var}` |
| i18n parity | Every key in `en.json` must exist in `ko.json` and `ja.json` |
| CJK overflow | `whitespace-nowrap` on badges and table headers |
| Recharts style | Import from `web/lib/chart-theme.ts` — never define chart constants in page files |
| Recharts formatters | Use `fmtMs`, `fmtCompact`, `fmtPct`, `fmtTemp` from `chart-theme.ts` |
| Accessibility | WCAG 2.1 AA: color+icon+text for status, `aria-label` on icon-only buttons, focus ring |
| Focus ring | `4px solid var(--vds-theme-border-focus)`, offset 4px (declared in `app/globals.css` overrides layer) |
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
Servers             -> /servers
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

| Property | Value |
|----------|-------|
| Width | `w-56` expanded / `w-14` collapsed; `transition-all duration-200` |
| Collapse state | `localStorage('nav-collapsed')` |
| Group state | `localStorage('nav-group-{id}')`, default open for `id: 'overview'` |
| Active detection | `isChildActive()` per child |
| Mobile | hamburger slide sidebar, `w-72`, backdrop close, auto-close on route change |

---

## DataTable Component (SSOT)

All data tables use `<DataTable>` from `web/components/data-table.tsx`.

```tsx
<DataTable minWidth="700px">
  <TableHeader>...</TableHeader>
  <TableBody>...</TableBody>
</DataTable>
```

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `minWidth` | `string` | `'600px'` | Minimum width before horizontal scroll |
| `footer` | `ReactNode` | -- | Optional footer (e.g. pagination) |

Base padding: `TableHead` `h-11 px-4`, `TableCell` `py-3 px-4`. First cell `pl-6`, last `pr-6`. Never override edge padding.

---

## State Management

- Server state: TanStack Query (`useQuery`, `useMutation`); local state: `useState` for modals
- No global client store (no Redux/Zustand)
- QueryClient config (`layout.tsx`): `staleTime: 30_000`, `retry: 1`, `refetchOnWindowFocus: false`
- `refetchOnWindowFocus: false` prevents burst refetch on tab re-focus and avoids racing the token refresh mutex
