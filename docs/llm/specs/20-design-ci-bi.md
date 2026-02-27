# Spec 20 — Veronex Design System & Brand Identity

> **SSOT** for all UI design decisions, token rules, and component conventions.
> Design Concept: **"Verde Nexus"** — Verde (light: "Platinum Signal") / Nexus (dark: "Obsidian Verde")
> Last updated: 2026-02-28

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
  Vero  = truth / precision (Latin "verus") + Verde (green, life, nature)
  Nexus = connection hub, network node (Latin "nexus")

  Together: "Verde Nexus" — a living network that grows and connects
  Domain: veronex.verobee.com
```

### 1.3 Design Concept — "Verde Nexus"

The design concept unites two pillars:

| Pillar | Keyword | Expression |
|--------|---------|------------|
| **Verde** | Life · Nature · Growth · Precision | Deep Ivy (light) — forest depth, trust, organic certainty |
| **Nexus** | Technology · Connection · Metal · Hub | Platinum Pearl bg, Titanium Silver text, hexagonal logo mark |

**Light mode "Platinum Signal"** — 세련된 도시적 금속감 + 깊은 숲의 안정감
- Metallic Pearl background meets Deep Ivy accent: clean, trustworthy, professional
- Zero pure white glare: Platinum Pearl (`#f2f4f2`) reduces long-session eye fatigue

**Dark mode "Obsidian Verde"** — 밤의 숲 + 바이오 광채
- Obsidian Deep background with Bio-Emerald accent: natural depth, luminous connectivity
- Green-black palette evokes night forest; emerald glow signals active state

### 1.4 Logo

| Element | Value |
|---------|-------|
| **Mark** | `HexLogo` — flat-top honeycomb hexagon SVG, 32×32 viewBox |
| **Symbol** | Hexagon = Nexus node (honeycomb = emergent network structure) |
| **File** | `web/components/nav.tsx` → inline SVG using CSS gradient vars |
| **Favicon** | `web/public/favicon.svg` |
| **Wordmark** | `web/public/logo.svg` — hex mark + "Veronex" text |

### 1.5 Logo Colors (theme-aware)

| Mode | Gradient start | Gradient end | Hex | Rationale |
|------|---------------|-------------|-----|-----------|
| **Light** | `#091e12` ivy-950 | `#0f3325` Deep Ivy | `--palette-logo-light-start/end` | Deep shadow → brand primary |
| **Dark** | `#047857` emerald-700 | `#10b981` Bio-Emerald | `--palette-logo-dark-start/end` | Rich depth → luminous emerald |

Wordmark text:
- Light: `fill="#0f3325"` (Deep Ivy — same as primary)
- Dark: rendered as SVG; nav sidebar text uses `text-foreground` via Tailwind

---

## 2. Color System

### 2.1 Architecture

`web/app/tokens.css` — single SSOT, 4-layer architecture:

```
Layer 0  @property           CSS type-safety + color transitions (initial = light-mode default)
Layer 1  --light-* / --dark-*   raw hex palette (never reference in components)
Layer 2  --theme-*           semantic tokens (light/dark switchable via [data-theme='dark'])
Layer 3  @theme inline       Tailwind utility generation (--color-*)
```

**Rules (enforced, zero violations tolerated):**
- Never hardcode hex in TSX. Use Tailwind utilities or `var(--theme-*)`.
- Never use `--light-*` / `--dark-*` in components.
- `@property` initial-value must always match light-mode defaults.
- New token flow: Layer 1 → Layer 2 → Layer 0 → Layer 3.

### 2.2 Light Theme — "Platinum Signal"

All primary text WCAG AAA (≥ 7:1) on their background.

| Role | Name | Hex | WCAG |
|------|------|-----|------|
| Page bg | Platinum Pearl | `#f2f4f2` | — |
| Card bg | Pure White | `#ffffff` | — |
| Elevated | — | `#edf0ed` | — |
| Hover | — | `#e4e8e4` | — |
| Text primary | Anthracite | `#141a14` | ~14.4:1 ✓ AAA |
| Text secondary | Slate Silver | `#334155` | ~10:1 ✓ AAA |
| Text dim | Metallic Slate | `#475569` | ~7.4:1 ✓ AAA |
| Text faint | — | `#64748b` | ~4.5:1 ✓ AA |
| Border subtle | — | `#e2e8e0` | — |
| Border default | — | `#cbd5d1` | — |
| **Brand primary** | Deep Ivy | `#0f3325` | **12.71:1 ✓ AAA** |
| Primary hover | Ivy Mid | `#164429` | — |
| Primary soft | Ivy Soft | `#1d6b44` | charts |
| Primary fg (button) | White | `#ffffff` | on Deep Ivy ✓ |
| Chart-1 | Ivy Soft | `#1d6b44` | — |

### 2.3 Dark Theme — "Obsidian Verde"

| Role | Name | Hex | WCAG |
|------|------|-----|------|
| Page bg | Obsidian Deep | `#080a09` | — |
| Card bg | Dark Graphite | `#111412` | — |
| Elevated | — | `#182019` | — |
| Hover | — | `#1e2820` | — |
| Text primary | Soft Platinum | `#e2e8e2` | ~14.2:1 ✓ AAA |
| Text secondary | Titanium Silver | `#94a3b8` | ~7.7:1 ✓ AAA |
| Text dim | — | `#64748b` | ~4.1:1 ✓ AA |
| Border subtle | — | `#1a2118` | — |
| Border default | — | `#2a3828` | — |
| **Brand primary** | Bio-Emerald | `#10b981` | **7.73:1 on page ✓ AAA** |
| Primary hover | Emerald-400 | `#34d399` | — |
| Primary soft | Emerald-600 | `#059669` | subtle bg |
| Primary fg (button) | Deep Dark | `#041f16` | on Bio-Emerald |
| Chart-1 | Bio-Emerald | `#10b981` | — |

> Note: Dark mode button contrast (`#041f16` on `#10b981`) = ~6.3:1 (AA). This is a deliberate
> brand choice — vivid emerald with deep dark inscription. Primary text on page/card remains AAA.

### 2.4 Status Colors

#### Light mode (WCAG AAA on white/Pearl bg)

| Status | Hex | WCAG | Token |
|--------|-----|------|-------|
| Success | `#065f46` | 7.67:1 ✓ AAA | `--theme-status-success` / `-fg` |
| Error | `#9f1239` | 8.02:1 ✓ AAA | `--theme-status-error` / `-fg` |
| Warning | `#78350f` | 9.06:1 ✓ AAA | `--theme-status-warning` / `-fg` |
| Info | `#1e40af` | 8.71:1 ✓ AAA | `--theme-status-info` / `-fg` |
| Cancelled | `#475569` | 7.59:1 ✓ AAA | `--theme-status-cancelled` |

#### Dark mode (vivid — not pastel)

| Status | Hex | Token |
|--------|-----|-------|
| Success | `#34d399` emerald-400 | `--theme-status-success` / `-fg` |
| Error | `#fb7185` rose-400 | `--theme-status-error` / `-fg` |
| Warning | `#fbbf24` amber-400 | `--theme-status-warning` / `-fg` |
| Info | `#60a5fa` blue-400 | `--theme-status-info` / `-fg` |

> **Policy**: Dark mode status colors must be vivid (not pastel/muted).

### 2.5 Accent Tokens

| Token | Light | Dark | Usage |
|-------|-------|------|-------|
| `--theme-accent-gpu` | `#0e7490` | `#22d3ee` | GPU/VRAM indicators |
| `--theme-accent-power` | `#78350f` | `#fbbf24` | Power/energy metrics |
| `--theme-accent-brand` | `#0f3325` | `#10b981` | Prompt labels, highlights |
| `--theme-focus-ring` | `#0f3325` | `#10b981` | Focus ring (matches primary) |

### 2.6 Tailwind Utilities (Layer 3)

All tokens exposed as `--color-*` in `@theme inline`. Key utilities:

```
bg-background        text-foreground        border-border
bg-card              text-card-foreground
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
text-accent-brand  bg-accent-brand
bg-surface-code    bg-chart-1 ... bg-chart-5
```

Opacity modifiers: `bg-status-success/15`, `border-status-error/30` — fully supported.

---

## 3. Typography

### 3.1 Font Stack

System font stack — no Google Fonts import (avoids CJK rendering issues):
```css
font-family: ui-sans-serif, system-ui, -apple-system, sans-serif;
```

> **Policy**: Do NOT import display/serif fonts. They break CJK rendering.

### 3.2 Type Scale

| Class | Size | Usage |
|-------|------|-------|
| `text-xs` | 12px | Metadata, timestamps, badges, mono labels |
| `text-sm` | 14px | Table rows, form labels, body |
| `text-base` | 16px | Standard body |
| `text-2xl font-bold tracking-tight` | 24px | **Page titles** (all pages) |

> **Policy**: All page `<h1>` use `text-2xl font-bold tracking-tight`. No decorative fonts.

---

## 4. Layout & Spacing

### 4.1 Layout Structure

Desktop:
```
┌──────────────────────────────────────────────┐
│  Sidebar (w-56 / w-14 collapsed)             │  Main content
│  bg-card  border-r border-border             │  bg-background
│  ────────────────────                        │  p-8  overflow-y-auto
│  HexLogo + "Veronex" + collapse [‹]          │
│  Nav links (icon + label / icon only)        │
│  ────────────────────                        │
│  v0.1.0  [🌐 EN ▾]  [🌙/☀]                 │
└──────────────────────────────────────────────┘
```

Mobile (< md breakpoint):
```
Mobile top bar (fixed, h-12, z-30):  ☰  [hex] Veronex
Sidebar: fixed inset-y-0 left-0 z-50 w-72  →  slide-in overlay
Backdrop: z-40 bg-black/50
```

### 4.2 Sidebar

| State | Width | Behavior |
|-------|-------|----------|
| Expanded | `md:w-56` | Logo + text + labels |
| Collapsed | `md:w-14` | Logo only, icon-only nav |
| Mobile | `w-72 fixed` | Slide overlay, close on route change |

- State: `localStorage('nav-collapsed')`
- Transition: `transition-all duration-200`

### 4.3 Component Spacing

| Element | Class |
|---------|-------|
| Page padding | `p-4 pt-16 md:p-8` |
| Section gap | `space-y-6` |
| Card padding | `p-5` or `p-6` |
| Form fields | `space-y-4`, label `space-y-1.5` |

### 4.4 Responsive Tables

All tables must have `overflow-x-auto` on wrapper + `min-w-[xxx]` on `<Table>`:

| Page | min-w |
|------|-------|
| `servers/page.tsx` | `min-w-[700px]` |
| `providers/page.tsx` OllamaTab | `min-w-[800px]` |
| `providers/page.tsx` GeminiTab | `min-w-[760px]` |
| `providers/page.tsx` OllamaSyncSection | `min-w-[600px]` |
| `keys/page.tsx` | `min-w-[700px]` |
| `job-table.tsx` | `min-w-[760px]` |

### 4.5 Border Radius

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
```

Fixed display order: `pending → running → completed → failed → cancelled`

### 5.2 Empty States

```tsx
<Card>
  <div className="p-8 text-center text-muted-foreground">{t('common.empty')}</div>
</Card>
```

### 5.3 Error States

```tsx
<Card className="border-destructive/50 bg-destructive/10">
  <CardContent className="p-6 text-destructive">
    <p className="font-semibold">{t('...')}</p>
    <p className="text-sm mt-1 opacity-80">{message}</p>
  </CardContent>
</Card>
```

### 5.4 Recharts Integration

```tsx
const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '8px',
  color: 'var(--theme-text-primary)',
  fontSize: '12px',
}
<XAxis tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} />
<Bar fill="var(--theme-primary)" />
```

### 5.5 Focus Ring

```css
*:focus-visible {
  outline: 4px solid var(--theme-focus-ring);
  outline-offset: 4px;
  border-radius: 2px;
}
```

---

## 6. Internationalization (i18n)

- **Library**: `react-i18next`
- **Locales**: `en` (default) · `ko` · `ja`
- **Detection**: `localStorage('hg-lang')` → `navigator.language` → `'en'`
- **Policy**: All user-visible strings via `t('key')` — zero hardcoded English in TSX.

---

## 7. Theme Switching

- Attribute: `document.documentElement.setAttribute('data-theme', 'dark')`
- Provider: `web/components/theme-provider.tsx` — `localStorage('hg-theme')`
- Flash prevention: inline `<script>` in `<head>` before React hydration
- Default: **light** mode

---

## 8. Tech Stack

| Layer | Choice |
|-------|--------|
| CSS framework | Tailwind v4 (CSS-first, no `tailwind.config.ts`) |
| PostCSS | `@tailwindcss/postcss` |
| Animations | `tw-animate-css` |
| Components | shadcn/ui (Radix UI primitives) |
| State | TanStack Query v5 (30s staleTime default) |
| Charts | Recharts (`var(--theme-*)` for all colors) |
| i18n | react-i18next (client-side, single namespace) |
| Theme | custom ThemeProvider (`data-theme` attribute) |

---

## 9. Change Log

| Version | Date | Change |
|---------|------|--------|
| 0.5.0 | 2026-02-28 | **Verde Nexus** design concept: "Platinum Signal" light (Deep Ivy `#0f3325`) + "Obsidian Verde" dark (Bio-Emerald `#10b981`); logo gradients updated both modes; mobile responsive nav; responsive tables |
| 0.4.0 | 2026-02-28 | Verde Signal light theme (Deep Forest `#16402e`), UUIDv7 policy |
| 0.3.0 | 2026-02-26 | Full design system: light + dark themes, token audit, vivid dark status, collapsible nav, i18n |
| 0.2.0 | 2026-02-25 | i18n (en/ko/ja), token system, status badges, Recharts CSS vars |
| 0.1.0 | 2026-02-24 | Initial dark theme |
