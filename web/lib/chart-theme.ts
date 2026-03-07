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

// ── Tooltip ─────────────────────────────────────────────────────────────────

/** Outer container style for every <Tooltip contentStyle={...}> */
export const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '8px',
  color: 'var(--theme-text-primary)',
  fontSize: '12px',
}

/**
 * <Tooltip labelStyle={...}>
 * Controls the category label (e.g. x-axis tick value shown at the top of the tooltip).
 * Without this, Recharts falls back to the browser default (typically black).
 */
export const TOOLTIP_LABEL_STYLE = { color: 'var(--theme-text-primary)' }

/**
 * <Tooltip itemStyle={...}>
 * Controls each series row (name + value) inside the tooltip.
 * Without this, Recharts uses the series fill color for the text, which may be unreadable.
 */
export const TOOLTIP_ITEM_STYLE = { color: 'var(--theme-text-primary)' }

// ── Axes ────────────────────────────────────────────────────────────────────

/** <XAxis tick={AXIS_TICK}> / <YAxis tick={AXIS_TICK}> */
export const AXIS_TICK = { fill: 'var(--theme-text-secondary)', fontSize: 11 }

// ── Legend ───────────────────────────────────────────────────────────────────

/** <Legend wrapperStyle={LEGEND_STYLE}> */
export const LEGEND_STYLE = { fontSize: '12px', color: 'var(--theme-text-secondary)' }

// ── Cursor overlays ─────────────────────────────────────────────────────────

/** <Tooltip cursor={CURSOR_FILL}> — area fill shown on bar chart hover */
export const CURSOR_FILL = { fill: 'var(--theme-bg-hover)' }

/** <Tooltip cursor={CURSOR_STROKE}> — vertical stroke shown on line chart hover */
export const CURSOR_STROKE = { stroke: 'var(--theme-border)' }

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
  return String(n)
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
export function fmtPct(n: number): string {
  return `${Math.round(n * 100)}%`
}

// ── Memory size formatter ─────────────────────────────────────────────────────

/** Format megabytes as a short string. Example: 2048 → "2.0 GB", 512 → "512 MB", 0 → "—" */
export function fmtMbShort(mb: number): string {
  if (mb === 0) return '—'
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}
