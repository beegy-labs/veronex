# Frontend Patterns — Design Tokens & Chart Theme

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## Shared Style Constants

All Tailwind class mappings live in `web/lib/constants.ts` — never duplicate across components:

| Constant | Keys | Purpose |
|----------|------|---------|
| `STATUS_STYLES` | completed, failed, cancelled, pending, running | Job status badge classes |
| `ROLE_STYLES` | system, user, assistant, tool | Chat message role badge classes |
| `FINISH_BG` | stop, length, error, cancelled | Finish reason badge classes |
| `FINISH_COLORS` | stop, length, error, cancelled | Finish reason chart colours |
| `PROVIDER_BADGE` | ollama, gemini | Provider type badge classes |
| `PROVIDER_COLORS` | ollama, gemini | Provider type chart colours |

Import from `@/lib/constants` — never duplicate style mappings across components.

## Chart Tooltip Style

`TOOLTIP_STYLE` in `web/lib/chart-theme.ts` is the SSOT for Recharts tooltip `contentStyle`:

```typescript
export const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '0.5rem',
  color: 'var(--theme-text-primary)',
}
```

Import and spread into `<Tooltip contentStyle={TOOLTIP_STYLE} />` — never inline tooltip styles.

## Chart Theme Formatters

All display formatters live in `web/lib/chart-theme.ts` — never define local formatting functions:

| Function | Example | Usage |
|----------|---------|-------|
| `fmtCompact(n)` | 1500 → "1.5K" | KPI cards, chart labels |
| `fmtMs(n)` | 1400 → "1.4s" | Latency display |
| `fmtMsNullable(n)` | null → "—" | Nullable latency |
| `fmtMsAxis(n)` | 86360 → "1.4m" | Chart Y-axis ticks |
| `fmtPct(n)` | 0.956 → "96%" | Success rates |
| `fmtMbShort(mb)` | 2048 → "2.0 GB" | VRAM display |

## Design Token System (4-Layer Architecture)

Token flow: `@property` → `tokens.css palette` → `tokens.css semantic (--theme-*)` → `@theme inline (Tailwind utilities)` → components

### Single Source of Truth

**All colors live in `web/app/tokens.css`. Zero exceptions.** Color changes touch exactly two layers:

| To change | Edit |
|-----------|------|
| A brand/raw color value | Layer 1 (`--palette-*`) only |
| What a semantic role maps to | Layer 2 (`--theme-*`) only |
| Add a new theme (e.g. brand X) | Layer 1 + Layer 2 only — **zero component code changes** |

If a color change requires touching `.tsx`, the policy has been violated somewhere — fix the violation, not the symptom.

### 4 Layers

| Layer | Purpose | Rule |
|-------|---------|------|
| 0. `@property --theme-*` | Type-safe `<color>` syntax + enables CSS color transitions | Register every `--theme-*` token here first |
| 1. `--palette-*` | Raw hex / OKLCH values | **Never reference from TSX — private to tokens.css** |
| 2. `--theme-*` | Semantic tokens switched via `[data-theme='dark'], .dark` | Theme switching happens here and nowhere else |
| 3. `@theme inline { --color-* }` | Tailwind v4 utility generation | Produces `bg-status-success`, `text-status-warning-fg` |

**Dark-mode selector**: use `[data-theme='dark'], .dark` dual selector for shadcn/third-party compatibility. Never rely on only one.

### Layer usage by context

| Context | Correct pattern | Wrong |
|---------|----------------|-------|
| Tailwind className | `bg-status-success`, `text-status-warning-fg` | `text-emerald-400`, `bg-green-600` |
| Inline `style={{}}` | `import { tokens } from '@/lib/design-tokens'` → `tokens.status.success` | `'var(--theme-status-success)'` raw string |
| SVG fill/stroke | `fill={tokens.status.info}` (JSX expression) | `fill="var(--theme-status-info)"` string |
| Chart gradient stopColor | `stopColor={tokens.brand.primary}` | `stopColor="var(--theme-primary)"` |
| Recharts fill/stroke | `fill={tokens.status.success}` | `fill="var(--theme-status-success)"` |
| 3rd-party CSS overrides | Dedicated `.css` file (e.g. `swagger-overrides.css`) + `import './swagger-overrides.css'` | Inline `<style>{`...`}</style>` blocks in `.tsx` |

### `tokens` module structure (`web/lib/design-tokens.ts`)

```typescript
import { tokens } from '@/lib/design-tokens'

// Backgrounds
tokens.bg.page | .card | .elevated | .hover

// Text
tokens.text.primary | .secondary | .bright | .dim | .faint

// Border
tokens.border.base | .subtle | .default

// Brand
tokens.brand.primary | .foreground | .ring | .focusRing

// Status (background / icon colour)
tokens.status.success | .error | .warning | .info | .cancelled

// Status foreground (text colour)
tokens.statusFg.success | .error | .warning | .info

// Accents
tokens.accent.gpu | .power | .brand

// Charts
tokens.chart.c1 | .c2 | .c3 | .c4 | .c5
```

### Token name rules

```
status-warning     ✓    status-warn     ✗
status-warning-fg  ✓    status-warn-fg  ✗
```

### Correct full example

```tsx
import { tokens } from '@/lib/design-tokens'

// Inline style — always tokens module
<span style={{ background: tokens.status.success }} />

// className — always Tailwind semantic utilities
<span className="bg-status-success text-status-success-fg" />

// SVG / Recharts
<Bar fill={tokens.status.info} />
<stop stopColor={tokens.brand.primary} stopOpacity={0.35} />

// Never
<span style={{ color: 'var(--theme-text-secondary)' }} />   // ✗ raw string
<span className="text-emerald-400" />                        // ✗ bypasses theme
<span style={{ color: '#065f46' }} />                        // ✗ hardcoded hex
```

### Adding a New Theme (procedure)

```
1. Layer 1 → add `--palette-{brand}-*` raw values (hex or OKLCH)
2. Layer 2 → add `[data-theme='{brand}'], .{brand} { --theme-*: ... }` block
3. Layer 0 / Layer 3 → no changes (automatic propagation)
4. web/components/theme-provider.tsx → add '{brand}' to theme option union
```

Component code is untouched. If any `.tsx` needs a change to support the new theme, the offending component is violating the single-source rule — fix the component, not the theme.

### OKLCH migration (Tailwind v4 default)

Tailwind v4 ships OKLCH palettes by default. Layer 1 values may be written in hex or OKLCH interchangeably; prefer OKLCH for wide-gamut (P3) displays. The semantic layer (Layer 2) and all component code remain color-space-agnostic.

---

