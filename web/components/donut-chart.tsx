'use client'

import { PieChart, Pie, Cell, Tooltip, ResponsiveContainer, Label } from 'recharts'
import {
  TOOLTIP_STYLE,
  TOOLTIP_LABEL_STYLE,
  TOOLTIP_ITEM_STYLE,
} from '@/lib/chart-theme'
import { tokens } from '@/lib/design-tokens'

// ── Types ───────────────────────────────────────────────────────────────────

export interface DonutSlice {
  name: string
  value: number
  fill: string
}

interface DonutChartProps {
  data: DonutSlice[]
  size?: number
  innerRadius?: number
  outerRadius?: number
  /** Formats tooltip values. Receives raw number, returns display string. */
  formatter?: (v: number) => string
  /** Text shown in the center hole (e.g. "75%", "1.2K"). */
  centerLabel?: string
  /** Smaller sub-text rendered below centerLabel. */
  centerSub?: string
}

// ── Component ────────────────────────────────────────────────────────────────

/**
 * DonutChart — shared donut/pie chart component.
 *
 * Uses chart-theme.ts constants for all styling so light/dark mode is
 * consistent across every instance. Never inline Recharts PieChart directly
 * in page files — use this component instead.
 *
 * SSOT: docs/llm/frontend/web-charts.md
 */
export function DonutChart({
  data,
  size = 160,
  innerRadius = 44,
  outerRadius = 68,
  formatter,
  centerLabel,
  centerSub,
}: DonutChartProps) {
  return (
    <ResponsiveContainer width={size} height={size}>
      <PieChart>
        <Pie
          data={data}
          dataKey="value"
          cx="50%"
          cy="50%"
          innerRadius={innerRadius}
          outerRadius={outerRadius}
          strokeWidth={0}
        >
          {data.map((slice) => (
            <Cell key={slice.name} fill={slice.fill} />
          ))}

          {centerLabel && (
            <Label
              content={({ viewBox }) => {
                const { cx, cy } = viewBox as { cx: number; cy: number }
                return (
                  <text textAnchor="middle" dominantBaseline="middle">
                    <tspan
                      x={cx}
                      y={centerSub ? cy - 9 : cy}
                      fill={tokens.text.primary}
                      fontSize={13}
                      fontWeight="700"
                    >
                      {centerLabel}
                    </tspan>
                    {centerSub && (
                      <tspan
                        x={cx}
                        y={cy + 11}
                        fill={tokens.text.secondary}
                        fontSize={10}
                      >
                        {centerSub}
                      </tspan>
                    )}
                  </text>
                )
              }}
            />
          )}
        </Pie>

        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          labelStyle={TOOLTIP_LABEL_STYLE}
          itemStyle={TOOLTIP_ITEM_STYLE}
          formatter={formatter ? (v) => formatter(Number(v)) : undefined}
        />
      </PieChart>
    </ResponsiveContainer>
  )
}
