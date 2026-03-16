/**
 * Chart Theme — SSOT for all Recharts styling.
 *
 * All values use CSS variables so they automatically adapt to light / dark mode.
 *
 * RULES:
 *  - Never define chart style constants inside page files.
 *  - Import from here and apply to every Recharts component.
 *  - When adding a new chart type, add its shared props here first.
 *
 * SSOT: docs/llm/frontend/web-charts.md
 */

import { tokens } from './design-tokens'

// ── Tooltip ─────────────────────────────────────────────────────────────────

/** Outer container style for every <Tooltip contentStyle={...}> */
export const TOOLTIP_STYLE = {
  backgroundColor: tokens.bg.card,
  border: `1px solid ${tokens.border.base}`,
  borderRadius: '8px',
  color: tokens.text.primary,
  fontSize: '12px',
}

/**
 * <Tooltip labelStyle={...}>
 * Controls the category label (e.g. x-axis tick value shown at the top of the tooltip).
 * Without this, Recharts falls back to the browser default (typically black).
 */
export const TOOLTIP_LABEL_STYLE = { color: tokens.text.primary }

/**
 * <Tooltip itemStyle={...}>
 * Controls each series row (name + value) inside the tooltip.
 * Without this, Recharts uses the series fill color for the text, which may be unreadable.
 */
export const TOOLTIP_ITEM_STYLE = { color: tokens.text.primary }

// ── Axes ────────────────────────────────────────────────────────────────────

/** <XAxis tick={AXIS_TICK}> / <YAxis tick={AXIS_TICK}> */
export const AXIS_TICK = { fill: tokens.text.secondary, fontSize: 11 }

/** Smaller axis tick for compact charts (history modals, sparklines). */
export const AXIS_TICK_SM = { fontSize: 10, fill: tokens.text.faint }

// ── Legend ───────────────────────────────────────────────────────────────────

/** <Legend wrapperStyle={LEGEND_STYLE}> */
export const LEGEND_STYLE = { fontSize: '12px', color: tokens.text.secondary }

// ── Cursor overlays ─────────────────────────────────────────────────────────

/** <Tooltip cursor={CURSOR_FILL}> — area fill shown on bar chart hover */
export const CURSOR_FILL = { fill: tokens.bg.hover }

/** <Tooltip cursor={CURSOR_STROKE}> — vertical stroke shown on line chart hover */
export const CURSOR_STROKE = { stroke: tokens.border.base }

// ── Compact number formatter ─────────────────────────────────────────────────

/**
 * Compact number for chart labels, KPI cards, and table cells where space is limited.
 * SSOT: use instead of local fmt() functions in page files.
 *
 * Examples: 1234 → "1.2K" | 1_500_000 → "1.5M" | 999 → "999"
 */
export function fmtCompact(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}K`
  return Number.isInteger(n) ? String(n) : n.toFixed(1)
}

// ── Duration / latency formatters ────────────────────────────────────────────

/**
 * Format a millisecond value for display (KPI cards, tooltips, table cells).
 * Tiers: ms → s → "Xm Xs" → "Xh Xm"
 *
 * Examples: 543 → "543ms" | 1400 → "1.4s" | 86360 → "1m 26s" | 5400000 → "1h 30m"
 */
export function fmtMs(n: number): string {
  if (n < 1_000)       return `${Math.round(n)}ms`
  if (n < 60_000)      return `${(n / 1_000).toFixed(1)}s`
  if (n < 3_600_000) {
    const m = Math.floor(n / 60_000)
    const s = Math.round((n % 60_000) / 1_000)
    return s > 0 ? `${m}m ${s}s` : `${m}m`
  }
  const h = Math.floor(n / 3_600_000)
  const m = Math.floor((n % 3_600_000) / 60_000)
  return m > 0 ? `${h}h ${m}m` : `${h}h`
}

/**
 * Compact formatter for chart Y-axis tick labels.
 * Single unit only, no decimals for sub-minute values.
 *
 * Examples: 543 → "543ms" | 1400 → "1s" | 86360 → "1.4m" | 5400000 → "1.5h"
 */
export function fmtMsAxis(n: number): string {
  if (n < 1_000)       return `${Math.round(n)}ms`
  if (n < 60_000)      return `${Math.round(n / 1_000)}s`
  if (n < 3_600_000)   return `${(n / 60_000).toFixed(1)}m`
  return `${(n / 3_600_000).toFixed(1)}h`
}

/**
 * Format a nullable millisecond value (e.g. job latency).
 * Returns "—" for null/undefined.
 */
export function fmtMsNullable(n: number | null | undefined): string {
  if (n == null) return '—'
  return fmtMs(n)
}

// ── Percentage formatter ──────────────────────────────────────────────────────

/** Format a 0..1 ratio as a rounded percentage string. Example: 0.956 → "96%" */
/** Format a percentage value (0–100 scale from backend) as "XX%". */
export function fmtPct(n: number): string {
  return `${Math.round(n)}%`
}

/** Format a percentage value with one decimal place. Example: 12.345 → "12.3%" */
export function fmtPct1(n: number): string {
  return `${n.toFixed(1)}%`
}

/** Format tokens-per-second for TPS display. Example: 23.456 → "23.46 tok/s", 0 → "—" */
export function fmtTps(n: number): string {
  if (n <= 0) return '—'
  return `${n.toFixed(2)} tok/s`
}

// ── Memory size formatters ────────────────────────────────────────────────────

/**
 * Format megabytes with binary prefix (GiB / MiB).
 * Used in server metrics cells and provider modals.
 * Example: 2048 → "2.0 GiB", 512 → "512 MiB"
 */
export function fmtMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GiB`
  return `${mb} MiB`
}

/** Format megabytes as a short string (decimal prefix). Example: 2048 → "2.0 GB", 512 → "512 MB", 0 → "—" */
export function fmtMbShort(mb: number): string {
  if (mb === 0) return '—'
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

// ── Temperature / Power / Cost formatters ─────────────────────────────────

/** Format temperature in Celsius. Example: 72.3 → "72°C", null → "—" */
export function fmtTemp(celsius: number | null | undefined): string {
  if (celsius == null) return '—'
  return `${celsius.toFixed(0)}°C`
}

/** Format power in watts. Example: 45.7 → "46W", null → "—" */
export function fmtPower(watts: number | null | undefined): string {
  if (watts == null) return '—'
  return `${watts.toFixed(0)}W`
}

/** Format ISO timestamp as HH:MM for chart axis labels. */
export function fmtTimeHHMM(iso: string): string {
  const d = new Date(iso)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Format USD cost. Example: 0.0012 → "$0.0012", 0 → "free", null → "—" */
export function fmtCost(usd: number | null | undefined): string {
  if (usd == null) return '—'
  if (usd === 0) return 'free'
  return `$${usd.toFixed(4)}`
}

/** Format USD cost with 6 decimal places — for detailed per-job views. Example: 0.000042 → "$0.000042", 0 → "free", null → "—" */
export function fmtCost6(usd: number | null | undefined): string {
  if (usd == null) return '—'
  if (usd === 0) return 'free'
  return `$${usd.toFixed(6)}`
}

/** Format kilowatt-hours for power displays. Example: 1.234 → "1.23 kWh", null → "—" */
export function fmtKwh(kwh: number | null | undefined): string {
  if (kwh == null) return '—'
  return `${kwh.toFixed(2)} kWh`
}
