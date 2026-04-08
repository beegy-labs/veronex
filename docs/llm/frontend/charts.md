# Web — Chart System

> SSOT | **Last Updated**: 2026-03-08
> DonutChart component: `frontend/charts-donut.md`

**Version**: recharts `^3.7.0` (upgraded from 2.x — no code migration needed).

All Recharts styling managed from a single source of truth. Never define chart style constants inside page files.

```
web/lib/chart-theme.ts          <- SSOT: all chart style constants
web/components/donut-chart.tsx  <- shared DonutChart component
```

---

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change tooltip background/border | `web/lib/chart-theme.ts` → `TOOLTIP_STYLE` | One change applies everywhere |
| Change axis tick color or size | `web/lib/chart-theme.ts` → `AXIS_TICK` | One change applies everywhere |
| Change legend text color | `web/lib/chart-theme.ts` → `LEGEND_STYLE` | One change applies everywhere |
| Add a donut/pie chart | Use `<DonutChart>` from `@/components/donut-chart` | See `charts-donut.md` |
| Add a bar/line/area chart | Import theme constants from `@/lib/chart-theme` | Apply to XAxis, YAxis, Tooltip, Legend |
| Add a new chart style constant | `web/lib/chart-theme.ts` | Export named const with JSDoc comment |
| Format numbers compactly | Use `fmtCompact(n)` from `chart-theme.ts` | 1234→"1.2K", 77.8→"77.8", 999→"999" |

---

## `web/lib/chart-theme.ts` — Constants Reference

```typescript
TOOLTIP_STYLE       -> contentStyle prop on <Tooltip>
TOOLTIP_LABEL_STYLE -> labelStyle prop on <Tooltip>   (category label text)
TOOLTIP_ITEM_STYLE  -> itemStyle prop on <Tooltip>    (series name + value rows)
AXIS_TICK           -> tick prop on <XAxis> / <YAxis>
LEGEND_STYLE        -> wrapperStyle prop on <Legend>
CURSOR_FILL         -> cursor prop on <Tooltip> for bar charts (area fill)
CURSOR_STROKE       -> cursor prop on <Tooltip> for line charts (vertical stroke)
```

Recharts does **not** inherit `contentStyle.color` for tooltip label or item rows — without explicit `labelStyle`/`itemStyle`, falls back to browser default (black, invisible on dark).

**Always use all three:**
```tsx
<Tooltip
  contentStyle={TOOLTIP_STYLE}
  labelStyle={TOOLTIP_LABEL_STYLE}
  itemStyle={TOOLTIP_ITEM_STYLE}
  cursor={CURSOR_FILL}
/>
```

---

## Tooltip `formatter` — Return String, Not `[value, '']`

| Return type | Tooltip renders |
|-------------|-----------------|
| `string` | `{originalName}: {string}` — correct |
| `[string, '']` | `: {string}` — series name lost (bug) |
| `[string, name]` | `{name}: {string}` — explicit override |

When only formatting value (not overriding name), return a **string**:

```tsx
// Correct
<Tooltip formatter={(v: number) => fmt(v)} />

// WRONG: name becomes empty
<Tooltip formatter={(v: number) => [fmt(v), '']} />

// OK: explicit override (e.g. i18n)
<Tooltip formatter={(v: number) => [ms(v), t('performance.avgLatency')]} />
```

---

## Bar / Line / Area Charts

Not wrapped in shared component — variety of series configurations makes wrapping impractical. Import and apply constants consistently:

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

## Dual Y-Axis Pattern

When two series have vastly different scales, use dual Y-axes:

```tsx
<YAxis yAxisId="left" tick={AXIS_TICK} axisLine={false} tickLine={false} width={40} tickFormatter={fmtCompact} />
<YAxis yAxisId="right" orientation="right" tick={AXIS_TICK} axisLine={false} tickLine={false} width={50} tickFormatter={fmtCompact} />
<Area yAxisId="left" dataKey="requests" ... />
<Area yAxisId="right" dataKey="tokens" ... />
```

Used in `usage/components/overview-tab.tsx` and `usage/components/by-key-tab.tsx`.

---

## Pages Using Charts

| Page | Chart Types |
|------|-------------|
| `web/app/usage/page.tsx` | DonutChart × 2, AreaChart (dual Y), BarChart |
| `web/app/overview/page.tsx` | AreaChart |
| `web/app/performance/page.tsx` | LineChart, BarChart |

---

## Color Rules

- **Always** use `var(--theme-*)` tokens — never hardcode hex or rgb.
- Semantic colors: `var(--theme-primary)`, `var(--theme-status-success)`, `var(--theme-status-error)`, `var(--theme-status-warning)`, `var(--theme-status-info)`.
- Text in charts: `var(--theme-text-primary)` (strong) or `var(--theme-text-secondary)` (muted).
