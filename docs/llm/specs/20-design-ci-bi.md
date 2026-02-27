# Spec 20 — Veronex Design System & Component Policy

> **SSOT** for all UI design decisions, token rules, and component conventions.
> Themes: "Veronex Clarity" (light) / "Veronex Night" (dark)
> Last updated: 2026-02-26

---

## 1. Brand Identity

### 1.1 Product

| Field | Value |
|-------|-------|
| **Product name** | Veronex |
| **Tagline** | LLM inference queue and routing dashboard |
| **Nature** | Admin dashboard — operator-facing, light/dark switchable |
| **Version** | v0.1.0 |

### 1.2 Name Concept

```
Veronex — dual meaning:
  Vero + Nexus  →  truth/precision + connection hub
  veronex.verobee.com

Logo mark: "V" with nexus node dots
  V  = convergence vertex (Vero — verified, true)
  ●  = three connection nodes at each vertex (Nexus — hub)
```

### 1.3 Logo Colors (theme-aware)

| Mode | Gradient start | Gradient end | Rationale |
|------|---------------|-------------|-----------|
| **Light** | `#003f4f` deep teal | `#00899e` ocean teal | Ocean depth precision |
| **Dark** | `#2dd4bf` bright teal | `#7de8e4` pale teal | Luminous connectivity |

CSS vars: `var(--theme-logo-start)`, `var(--theme-logo-end)` — defined in `tokens.css` Layer 2,
override per theme in `[data-theme='dark']`.

File: `web/components/nav.tsx` → inline `VXLogo` SVG (28×28)

---

## 2. Color System

### 2.1 Architecture

`web/app/tokens.css` — single SSOT, 4-layer architecture:

```
Layer 0  @property           CSS type-safety + color transitions
Layer 1  --palette-* / --light-* / --dark-*   raw hex (never reference in components)
Layer 2  --theme-*           semantic tokens (light/dark switchable via [data-theme='dark'])
Layer 3  @theme inline       Tailwind utility generation (--color-*)
```

**Rules (enforced, zero violations tolerated):**
- Never hardcode hex in TSX. Use Tailwind utilities or `var(--theme-*)`.
- Never use `--palette-*` / `--light-*` / `--dark-*` in components.
- `@property` initial-value must always match light mode defaults.
- New token flow: Layer 1 → Layer 2 → Layer 0 → Layer 3 (if needed as Tailwind utility).

### 2.2 Light Theme — "Veronex Clarity"

All text colors WCAG AAA (7:1+) on their background.

| Role | Hex | Token |
|------|-----|-------|
| Page bg | `#f6f8fa` | `--theme-bg-page` |
| Card bg | `#ffffff` | `--theme-bg-card` |
| Elevated | `#edf1f7` | `--theme-bg-elevated` |
| Hover | `#e2e8f2` | `--theme-bg-hover` |
| Text primary | `#0d1117` | `--theme-text-primary` (~21:1 ✓ AAA) |
| Text secondary | `#24292f` | `--theme-text-secondary` (~13:1 ✓ AAA) |
| Text bright | `#000000` | `--theme-text-bright` |
| Text dim | `#4b5563` | `--theme-text-dim` (7.6:1 ✓ AAA) |
| Text faint | `#6b7280` | `--theme-text-faint` (decorative) |
| Border subtle | `#d0d7de` | `--theme-border-subtle` / `--theme-border` |
| Border default | `#8c959f` | `--theme-border-default` |
| **Brand primary** | `#005f73` | `--theme-primary` (deep ocean teal, 7.3:1 ✓ AAA) |
| Primary fg | `#ffffff` | `--theme-primary-foreground` |
| Destructive | `#b91c1c` | `--theme-destructive` |

### 2.3 Dark Theme — "Veronex Night"

| Role | Hex | Token |
|------|-----|-------|
| Page bg | `#0d1117` | `--theme-bg-page` |
| Card bg | `#161b22` | `--theme-bg-card` |
| Elevated | `#1c2128` | `--theme-bg-elevated` |
| Hover | `#22272e` | `--theme-bg-hover` |
| Text primary | `#e6edf3` | `--theme-text-primary` (16:1 ✓ AAA) |
| Text secondary | `#adbac7` | `--theme-text-secondary` (9:1 ✓ AAA) |
| Text bright | `#f0f6fc` | `--theme-text-bright` |
| Text dim | `#768390` | `--theme-text-dim` |
| Text faint | `#444c56` | `--theme-text-faint` |
| Border subtle | `#2d333b` | `--theme-border` |
| Border default | `#444c56` | `--theme-border-default` |
| **Brand primary** | `#2dd4bf` | `--theme-primary` (bright teal, 10.2:1 ✓ AAA) |
| Primary fg | `#0d1117` | `--theme-primary-foreground` |
| Destructive | `#fca5a5` | `--theme-destructive` |

### 2.4 Status Colors

#### Light mode (WCAG AAA on white/light bg)
| Status | Base hex | Fg hex | Token pair |
|--------|---------|--------|------------|
| Success | `#166534` | `#14532d` | `--theme-status-success` / `-fg` |
| Error | `#b91c1c` | `#991b1b` | `--theme-status-error` / `-fg` |
| Warning | `#92400e` | `#78350f` | `--theme-status-warning` / `-fg` |
| Info | `#1e40af` | `#1e3a8a` | `--theme-status-info` / `-fg` |
| Cancelled | `#8c959f` | — | `--theme-status-cancelled` |

#### Dark mode (vivid — not pastel)
| Status | Base hex | Token |
|--------|---------|-------|
| Success | `#4ade80` | `--theme-status-success` / `-fg` |
| Error | `#f87171` | `--theme-status-error` / `-fg` |
| Warning | `#fbbf24` | `--theme-status-warning` / `-fg` |
| Info | `#60a5fa` | `--theme-status-info` / `-fg` |

> **Policy**: Dark mode status colors must be vivid (not pastel/muted).
> Pastel values (#81c784, #ef9a9a, #ffe082, #90caf9) are explicitly rejected.

### 2.5 Accent & Extended Tokens

| Token | Light | Dark | Usage |
|-------|-------|------|-------|
| `--theme-accent-gpu` | `#6d28d9` | `#a78bfa` | GPU/VRAM indicators, Gemini icons |
| `--theme-accent-power` | `#b45309` | `#fbbf24` | Power/energy metrics |
| `--theme-accent-brand` | `#1d4ed8` | `#60a5fa` | Brand accent (prompt labels) |
| `--theme-surface-code` | `--light-bg-elevated` | `--dark-bg-elevated` | Code blocks, monospace |
| `--theme-focus-ring` | `#005f73` | `#2dd4bf` | Focus ring (same as primary) |

### 2.6 Tailwind Utilities (Layer 3)

All tokens exposed as `--color-*` in `@theme inline`. Available utilities:

```
bg-background        text-foreground        border-border
bg-card              text-card-foreground   border-input
bg-primary           text-primary-foreground
bg-muted             text-muted-foreground
bg-secondary         text-secondary-foreground
bg-accent            text-accent-foreground
bg-destructive       text-destructive-foreground

bg-status-success    text-status-success-fg    border-status-success
bg-status-error      text-status-error-fg      border-status-error
bg-status-warning    text-status-warning-fg    border-status-warning
bg-status-info       text-status-info-fg       border-status-info
bg-status-cancelled

text-text-bright   text-text-dim   text-text-faint

text-accent-gpu    text-accent-power    text-accent-brand
bg-accent-gpu      bg-accent-power      bg-accent-brand
bg-surface-code    text-surface-code

bg-chart-1 ... bg-chart-5
```

Opacity modifiers: `bg-status-success/15`, `border-status-error/30` etc. — fully supported.

---

## 3. Typography

### 3.1 Font Stack

System font stack — no Google Fonts import (avoids CJK rendering issues):
```css
font-family: ui-sans-serif, system-ui, -apple-system, sans-serif;
```

> **Policy**: Do NOT import display/serif fonts (Playfair Display etc.).
> They break CJK rendering (Korean/Japanese character spacing and fallback).

### 3.2 Type Scale

| Class | Size | Usage |
|-------|------|-------|
| `text-xs` | 12px | Metadata, timestamps, badges, mono labels |
| `text-sm` | 14px | Table rows, form labels, body |
| `text-base` | 16px | Standard body |
| `text-2xl font-bold tracking-tight` | 24px | **Page titles** (all pages) |

> **Policy**: All page `<h1>` titles use `text-2xl font-bold tracking-tight`.
> Never use `font-serif`, `italic`, or decorative font classes on page headings.

---

## 4. Layout & Spacing

### 4.1 Layout Structure

```
┌──────────────────────────────────────────────┐
│  Sidebar (w-56 / w-14 collapsed)             │  Main content
│  bg-card  border-r                           │  bg-background
│  ────────────────────                        │  p-8  overflow-y-auto
│  Logo + "InferQ" + collapse button  [‹]      │
│  Nav links (icon + label / icon only)        │
│  ────────────────────                        │
│  v0.1.0  [🌐 EN ▾]  [🌙/☀]                 │
└──────────────────────────────────────────────┘
```

### 4.2 Sidebar Collapse Policy

- State persisted: `localStorage('nav-collapsed')` = `'true'` | `'false'`
- **Expanded** (`w-56`): logo + text, nav label + icon, footer with lang/theme
- **Collapsed** (`w-14`): logo click expands, nav icons only with tooltip titles
- Transition: `transition-all duration-200`
- Lang switcher hidden when collapsed; theme toggle always visible

### 4.3 Component Spacing

| Element | Class |
|---------|-------|
| Page padding | `p-8` |
| Section gap | `space-y-6` |
| Card padding | `p-5` or `p-6` |
| Form fields | `space-y-4`, label `space-y-1.5` |
| Table cells | `px-4 py-3` (shadcn default) |

### 4.4 Border Radius

| Token | Value | Usage |
|-------|-------|-------|
| `--radius` | `0.5rem` (8px) | base |
| `rounded-md` | 6px | buttons, inputs, nav items |
| `rounded-lg` | 8px | dialogs |
| `rounded-xl` | 12px | cards |

---

## 5. Component Conventions

### 5.1 Status Badges

```tsx
const STATUS_CLASSES: Record<string, string> = {
  completed: 'bg-status-success/15 text-status-success-fg border-status-success/30',
  failed:    'bg-status-error/15   text-status-error-fg   border-status-error/30',
  pending:   'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  running:   'bg-status-info/15    text-status-info-fg    border-status-info/30',
  cancelled: 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30',
}

<Badge variant="outline" className={STATUS_CLASSES[status]}>
  {t(`jobs.statuses.${status}`)}
</Badge>
```

### 5.2 Status Display Order

Fixed order for all status breakdowns (charts, tables, filters):
```
pending → running → completed → failed → cancelled
```
Never derive order from API response object keys (`Object.entries` order is non-deterministic).

### 5.3 Empty States

```tsx
<Card>
  <div className="p-8 text-center text-muted-foreground">{t('common.empty')}</div>
</Card>
```

### 5.4 Error States

```tsx
<Card className="border-destructive/50 bg-destructive/10">
  <CardContent className="p-6 text-destructive">
    <p className="font-semibold">{t('...')}</p>
    <p className="text-sm mt-1 opacity-80">{message}</p>
  </CardContent>
</Card>
```

### 5.5 Recharts Integration

```tsx
// Tooltip style — always use CSS vars
const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '8px',
  color: 'var(--theme-text-primary)',
  fontSize: '12px',
}

// Axis ticks
<XAxis tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} />

// Series colors — use var(--theme-*), never hardcoded hex
<Line stroke="var(--theme-status-info)" />
<Bar fill="var(--theme-primary)" />
```

### 5.6 Focus Ring

```css
*:focus-visible {
  outline: 4px solid var(--theme-focus-ring);
  outline-offset: 4px;
  border-radius: 2px;
}
```

---

## 6. Internationalization (i18n)

### 6.1 Architecture

```
web/
├── i18n/
│   ├── config.ts       locales[], localeLabels{}, defaultLocale, localStorageKey
│   └── index.ts        i18next init + useTranslation re-export
├── messages/
│   ├── en.json         English (default)
│   ├── ko.json         한국어
│   └── ja.json         日本語
└── components/
    ├── i18n-provider.tsx   wraps app with i18next provider
    └── language-switcher.tsx  (legacy — use nav inline switcher)
```

- **Library**: `react-i18next`
- **Detection order**: `localStorage('hg-lang')` → `navigator.language` → `'en'`
- **Namespace**: single `translation` (admin tool, no code splitting)

### 6.2 Supported Locales

| Code | Language | Label |
|------|----------|-------|
| `en` | English | EN |
| `ko` | Korean | 한국어 |
| `ja` | Japanese | 日本語 |

### 6.3 Scalability Policy

Adding a new language requires only:
1. Add `'xx'` to `locales` array in `i18n/config.ts`
2. Add label to `localeLabels` in `i18n/config.ts`
3. Add `web/messages/xx.json` with all keys
4. Import and register in `i18n/index.ts`

The Nav language switcher uses a `<Select>` component that renders all `locales` dynamically — no code change needed in the UI.

### 6.4 Key Naming Convention

```jsonc
{
  "nav":         { "overview": "...", "jobs": "...", ... },
  "common":      { "loading": "...", "save": "...", "cancel": "...", ... },
  "overview":    { "title": "...", ... },
  "jobs":        { "title": "...", "statuses": { "pending": "...", ... }, ... },
  "keys":        { ... },
  "backends":    { "tabs": { "servers": "...", "ollama": "...", "gemini": "..." }, ... },
  "usage":       { ... },
  "performance": { ... },
  "test":        { ... },
  "metrics":     { ... }
}
```

### 6.5 i18n Compliance

- **All** user-visible strings must use `t('key')` — no hardcoded English in TSX.
- Component receiving `t` must call `const { t } = useTranslation()` locally.
- Date/number formatting: use `toLocaleString(undefined, ...)` (respects browser locale).

---

## 7. Theme Switching

### 7.1 Mechanism

- Attribute-based: `document.documentElement.setAttribute('data-theme', 'dark')`
- CSS selectors: `:root, [data-theme='light']` (default) and `[data-theme='dark']`
- Provider: `web/components/theme-provider.tsx` — `localStorage('hg-theme')`
- Flash prevention: inline script in `<head>` before React hydrates:
  ```html
  <script>(function(){try{var t=localStorage.getItem('hg-theme');
  if(t==='dark'){document.documentElement.setAttribute('data-theme','dark');}}
  catch(e){}})();</script>
  ```
- Default: **light** mode

### 7.2 Toggle UI

- Position: Nav footer, rightmost button
- Icon: `<Sun>` (when dark → click for light) / `<Moon>` (when light → click for dark)

---

## 8. Tech Stack

| Layer | Choice | Notes |
|-------|--------|-------|
| CSS framework | Tailwind v4 | CSS-first, no `tailwind.config.ts` |
| PostCSS | `@tailwindcss/postcss` | sole plugin |
| Animations | `tw-animate-css` | replaces `tailwindcss-animate` |
| Components | shadcn/ui | Radix UI primitives |
| State | TanStack Query v5 | server state, 30s staleTime default |
| Charts | Recharts | `var(--theme-*)` for all colors |
| i18n | react-i18next | client-side, single namespace |
| Theme | custom ThemeProvider | `data-theme` attribute |

---

## 9. Change Log

| Version | Date | Change |
|---------|------|--------|
| 0.3.0 | 2026-02-26 | Full girok design system: light + dark themes, token audit (0 violations), vivid dark status colors, logo theme-aware gradient, collapsible nav, i18n Select (N-lang scalable), job status fixed order |
| 0.2.0 | 2026-02-25 | i18n (en/ko/ja), token system, status badges, Recharts CSS vars |
| 0.1.0 | 2026-02-24 | Initial dark theme |
