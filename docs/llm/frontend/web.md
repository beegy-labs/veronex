# Web — Brand, Design System & Architecture

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
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
| Recharts | `var(--theme-*)` for all fill/stroke/tick |
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

## Responsive Tables

All tables use `overflow-x-auto` on the parent `<CardContent>` and a `min-w-[xxx]` on `<Table>` to prevent column collapse on small screens:

| Page / Component | min-w |
|-----------------|-------|
| `servers/page.tsx` ServersTable | `min-w-[700px]` |
| `providers/page.tsx` OllamaTab | `min-w-[800px]` |
| `providers/page.tsx` GeminiTab | `min-w-[760px]` |
| `providers/page.tsx` OllamaSyncSection model table | `min-w-[600px]` |
| `keys/page.tsx` | `min-w-[700px]` |
| `components/job-table.tsx` | `min-w-[760px]` |

> **Rule**: When adding a new table, always set `overflow-x-auto` on the wrapper and `min-w-[xxx]` on `<Table>` matching the column count (≈100px per column).

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

## Adding a New Provider (e.g. OpenAI)

1. Add entry to `navItems[].children` in `nav.tsx` (under `providers` group)
2. Add `section === 'openai'` branch in `providers/page.tsx` → new `<OpenAITab>`
3. Add i18n key `nav.openai` + tab strings to all 3 message files
4. Extend `BackendType` enum in Rust + add adapter in `infrastructure/outbound/`
5. Update `docs/llm/backend/backends-ollama.md` + `docs/llm/backend/openai.md`
6. Create `docs/llm/frontend/web-providers.md` section for the new tab
