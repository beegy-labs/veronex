import type { Timezone } from '@/components/timezone-provider'

/**
 * Centralized date formatters — all accept an IANA Timezone string.
 * Backend always returns ISO 8601 UTC; the user's selected timezone
 * is applied here on the client only.
 */

/** "Mar 1, 12:34:56" — job detail, audit events */
export function fmtDatetime(iso: string, tz: Timezone): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
    timeZone: tz,
  }).format(new Date(iso))
}

/** "Mar 1, 12:34" — dashboard recent jobs, providers synced */
export function fmtDatetimeShort(iso: string, tz: Timezone): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit',
    timeZone: tz,
  }).format(new Date(iso))
}

/** "Mar 1, 2026" — API keys, backend registered_at */
export function fmtDateOnly(iso: string, tz: Timezone): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short', day: 'numeric', year: 'numeric',
    timeZone: tz,
  }).format(new Date(iso))
}

/** "1,234,567" — integer count with comma separators (SSOT for all numeric counts) */
export function fmtNumber(n: number): string {
  return new Intl.NumberFormat('en-US').format(n)
}

/** "3/1 14h" — hourly chart x-axis labels */
export function fmtHourLabel(iso: string, tz: Timezone): string {
  const parts = new Intl.DateTimeFormat('en-US', {
    month: 'numeric', day: 'numeric', hour: 'numeric',
    timeZone: tz, hour12: false,
  }).formatToParts(new Date(iso))
  const month = parts.find(p => p.type === 'month')?.value ?? ''
  const day   = parts.find(p => p.type === 'day')?.value   ?? ''
  const hour  = parts.find(p => p.type === 'hour')?.value  ?? ''
  return `${month}/${day} ${hour.padStart(2, '0')}h`
}
