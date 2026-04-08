# Web — DonutChart Component

> SSOT | **Last Updated**: 2026-03-08
> Chart system, constants, bar/line patterns: `frontend/charts.md`

---

## `web/components/donut-chart.tsx` — DonutChart

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `data` | `DonutSlice[]` | required | Array of `{ name, value, fill }` |
| `size` | `number` | `160` | Width and height in px |
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
  fill: string   // CSS color — always use var(--theme-*) tokens
}
```

### Usage

```tsx
import { DonutChart } from '@/components/donut-chart'

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
// WRONG: Never inline PieChart in page files
<PieChart>
  <Pie ...>
    <Cell fill="#333" />   // WRONG: hardcoded color
  </Pie>
  <Tooltip contentStyle={TOOLTIP_STYLE} />  // WRONG: missing labelStyle / itemStyle
</PieChart>

// Always use DonutChart
<DonutChart data={slices} size={160} formatter={fmt} />
```

### Pages Using DonutChart

| Page | Usage |
|------|-------|
| `web/app/usage/page.tsx` | DonutChart × 2 (token distribution) |
