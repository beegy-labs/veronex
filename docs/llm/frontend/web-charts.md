# Web — Chart System

> SSOT | **Last Updated**: 2026-03-02 (rev: recharts 3.7 — no breaking changes for this codebase; formatter rule unchanged)

## Overview

**Version**: recharts `^3.7.0` (upgraded from 2.x — no code migration needed for this codebase).

> **Recharts 3 breaking changes that do NOT affect this project:**
> - `CartesianGrid` now requires `xAxisId`/`yAxisId` — not used here
> - `TooltipProps` → `TooltipContentProps` — no custom tooltip type annotations used
> - `accessibilityLayer` defaults to `true` — no visual change

All Recharts styling is managed from a single source of truth.
Never define chart style constants inside page files.

```
web/lib/chart-theme.ts          ← SSOT: all chart style constants
web/components/donut-chart.tsx  ← shared DonutChart component
```

---

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change tooltip background/border | `web/lib/chart-theme.ts` → `TOOLTIP_STYLE` | One change applies everywhere |
| Change axis tick color or size | `web/lib/chart-theme.ts` → `AXIS_TICK` | One change applies everywhere |
| Change legend text color | `web/lib/chart-theme.ts` → `LEGEND_STYLE` | One change applies everywhere |
| Add a donut/pie chart | Use `<DonutChart>` from `@/components/donut-chart` | Never inline `<PieChart>` in pages |
| Add a bar/line/area chart | Import theme constants from `@/lib/chart-theme` | Apply to XAxis, YAxis, Tooltip, Legend |
| Add a new chart style constant | `web/lib/chart-theme.ts` | Export a named const with a JSDoc comment |

---

## `web/lib/chart-theme.ts` — Constants Reference

```typescript
// Tooltip container
TOOLTIP_STYLE       → contentStyle prop on <Tooltip>
TOOLTIP_LABEL_STYLE → labelStyle prop on <Tooltip>   (category label text)
TOOLTIP_ITEM_STYLE  → itemStyle prop on <Tooltip>    (series name + value rows)

// Axes
AXIS_TICK           → tick prop on <XAxis> / <YAxis>

// Legend
LEGEND_STYLE        → wrapperStyle prop on <Legend>

// Cursor overlays
CURSOR_FILL         → cursor prop on <Tooltip> for bar charts (area fill)
CURSOR_STROKE       → cursor prop on <Tooltip> for line charts (vertical stroke)
```

### Why `labelStyle` and `itemStyle` Are Required

Recharts does **not** inherit `contentStyle.color` for:
- The tooltip label (the category / x-axis value shown at the top)
- The item text rows (series name + formatted value)

Without explicit `labelStyle` / `itemStyle`, Recharts falls back to the browser default
(`color: black`), which is invisible on dark backgrounds.

**Always use all three:**
```tsx
<Tooltip
  contentStyle={TOOLTIP_STYLE}
  labelStyle={TOOLTIP_LABEL_STYLE}
  itemStyle={TOOLTIP_ITEM_STYLE}
  cursor={CURSOR_FILL}         // for bar charts
  // cursor={CURSOR_STROKE}    // for line charts
/>
```

---

### Tooltip `formatter` — Return String, Not `[value, '']`

Recharts `formatter` signature: `(value, name, props) => displayValue | [displayValue, displayName]`

| Return type | Tooltip renders |
|-------------|-----------------|
| `string` | `{originalName}: {string}` ← **correct** |
| `[string, '']` | `: {string}` ← series name lost (bug) |
| `[string, name]` | `{name}: {string}` ← explicit override |

**Rule**: When you only want to format the value (not override the name), return a **string**:

```tsx
// ✅ Correct — original series name is preserved
<Tooltip formatter={(v: number) => fmt(v)} />

// ❌ Wrong — name becomes empty, shows ": 6" instead of "error: 6"
<Tooltip formatter={(v: number) => [fmt(v), '']} />

// ✅ OK — explicit name override (e.g. for i18n)
<Tooltip formatter={(v: number) => [ms(v), t('performance.avgLatency')]} />
```

This rule applies to **all chart types** including `DonutChart` (via `formatter` prop).

---

## `web/components/donut-chart.tsx` — DonutChart

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `data` | `DonutSlice[]` | required | Array of `{ name, value, fill }` |
| `size` | `number` | `160` | Width and height in px (`ResponsiveContainer` uses fixed size) |
| `innerRadius` | `number` | `44` | Inner hole radius |
| `outerRadius` | `number` | `68` | Outer ring radius |
| `formatter` | `(v: number) => string` | — | Tooltip value formatter |
| `centerLabel` | `string` | — | Bold text in center hole |
| `centerSub` | `string` | — | Muted sub-text below centerLabel |

### DonutSlice

```typescript
interface DonutSlice {
  name: string   // shown in tooltip label
  value: number  // data value
  fill: string   // CSS color string — always use var(--theme-*) tokens
}
```

### Usage

```tsx
import { DonutChart } from '@/components/donut-chart'

// Basic
<DonutChart
  data={[
    { name: 'Prompt',     value: 800, fill: 'var(--theme-primary)' },
    { name: 'Completion', value: 200, fill: 'var(--theme-status-info)' },
  ]}
  size={160}
  formatter={(v) => `${(v / 1000).toFixed(1)}K`}
/>

// With center label
<DonutChart
  data={slices}
  size={160}
  centerLabel="75%"
  centerSub="of total"
/>
```

### What NOT to do

```tsx
// ❌ Never inline PieChart in page files
<PieChart>
  <Pie ...>
    <Cell fill="#333" />   // ❌ hardcoded color
  </Pie>
  <Tooltip contentStyle={TOOLTIP_STYLE} />  // ❌ missing labelStyle / itemStyle
</PieChart>

// ✅ Always use DonutChart
<DonutChart data={slices} size={160} formatter={fmt} />
```

---

## Bar / Line / Area Charts — Pattern

These chart types are not wrapped in a shared component (the variety of series
configurations makes wrapping impractical). Instead, import theme constants and
apply them consistently.

```tsx
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
} from '@/lib/chart-theme'

<BarChart data={data} barGap={2}>
  <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
  <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
  <Tooltip
    contentStyle={TOOLTIP_STYLE}
    labelStyle={TOOLTIP_LABEL_STYLE}
    itemStyle={TOOLTIP_ITEM_STYLE}
    cursor={CURSOR_FILL}
  />
  <Legend wrapperStyle={LEGEND_STYLE} />
  <Bar dataKey="count" fill="var(--theme-primary)" radius={[3, 3, 0, 0]} />
</BarChart>
```

---

## Pages Using Charts

| Page | Chart Types | Notes |
|------|-------------|-------|
| `web/app/usage/page.tsx` | DonutChart × 2, AreaChart, BarChart | Uses `DonutChart` component |
| `web/app/overview/page.tsx` | AreaChart | |
| `web/app/performance/page.tsx` | LineChart, BarChart | |

---

## Color Rules

- **Always** use `var(--theme-*)` tokens — never hardcode hex or rgb values.
- Semantic colors for series: `var(--theme-primary)`, `var(--theme-status-success)`,
  `var(--theme-status-error)`, `var(--theme-status-warning)`, `var(--theme-status-info)`.
- Text in charts: `var(--theme-text-primary)` (strong) or `var(--theme-text-secondary)` (muted).
