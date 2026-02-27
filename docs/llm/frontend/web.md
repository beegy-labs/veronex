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
- **Favicon**: `web/public/favicon.svg` — violet gradient `#4c1d95 → #7c3aed`
- **Wordmark**: `web/public/logo.svg` — hex mark + "Veronex" text in `#5b21b6`

---

## Design Theme — "Nexus Signal"

| | Light "Signal" | Dark "Signal Dark" |
|---|---|---|
| Page bg | `#f8f9fb` | `#09090f` |
| Card bg | `#ffffff` | `#0c0e17` |
| Primary | `#5b21b6` violet-800 (8.97:1 AAA) | `#a78bfa` violet-400 (7.07:1 AAA) |
| Text primary | `#0d1117` ~19:1 AAA | `#e8ecf5` ~16.8:1 AAA |
| Text secondary | `#334155` ~10:1 AAA | `#94a3b8` ~7.8:1 AAA |
| Border | `#e2e8f0` | `#1c2030` |

WCAG targets: Primary ≥7:1 (AAA), body text AAA, status colors AAA both modes.
Dark status colors: `#34d399` / `#fb7185` / `#fbbf24` / `#60a5fa`

---

## tokens.css — 4-Layer Token Architecture

```css
/* Layer 0: @property — type safety + CSS transition support */
@property --theme-primary { syntax: '<color>'; ... }

/* Layer 1: --palette-* raw hex (NEVER use in components) */
--palette-violet-800: #5b21b6;

/* Layer 2: --theme-* semantic (switches via [data-theme='dark']) */
--theme-primary: var(--palette-violet-800);     /* light */
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
