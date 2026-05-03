import { tokens } from './design-tokens'

/** Single source of truth for the Veronex backend URL. */
export const BASE_API_URL =
  process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

/** Provider type identifiers — single source of truth. */
export const PROVIDER_OLLAMA = 'ollama' as const
export const PROVIDER_GEMINI = 'gemini' as const

/** Provider type → Tailwind badge class. */
export const PROVIDER_BADGE: Record<string, string> = {
  ollama: 'vds-bg-primary/10 vds-text-primary vds-border-primary/30',
  gemini: 'vds-bg-info/10 vds-text-info vds-border-info/30',
}

/** Provider type → CSS custom-property chart colour. */
export const PROVIDER_COLORS: Record<string, string> = {
  ollama: tokens.brand.primary,
  gemini: tokens.status.info,
}

/** Job status → chart/SVG colour (CSS variable). */
export const JOB_STATUS_COLORS: Record<string, string> = {
  completed: tokens.status.success,
  failed:    tokens.status.error,
  running:   tokens.status.info,
  cancelled: tokens.status.cancelled,
  pending:   tokens.status.warning,
}

/** Finish reason → chart colour. */
export const FINISH_COLORS: Record<string, string> = {
  stop:      tokens.status.success,
  length:    tokens.status.warning,
  error:     tokens.status.error,
  cancelled: tokens.text.secondary,
}

/** Finish reason → Tailwind badge class. */
export const FINISH_BG: Record<string, string> = {
  stop:      'vds-bg-success/15 vds-text-success vds-border-success/30',
  length:    'vds-bg-warning/15 vds-text-warning vds-border-warning/30',
  error:     'vds-bg-error/15 vds-text-error vds-border-error/30',
  cancelled: 'vds-bg-muted vds-text-dim vds-border-subtle',
}

// ── Image defaults ──────────────────────────────────────────────────────────

/** Default max images per request (matches Rust LabSettings::default()). */
export const DEFAULT_MAX_IMAGES = 4

/** Upper bound for max_images_per_request setting. */
export const MAX_IMAGES_LIMIT = 20

/** Default max image base64 bytes (matches Rust LabSettings::default()). */
export const DEFAULT_MAX_IMAGE_B64_BYTES = 2 * 1024 * 1024

/** Max file size before compression (UX guard, not a security boundary). */
export const MAX_FILE_BYTES = 10 * 1024 * 1024

/** Delay (ms) before invalidating queries after a sync operation. */
export const SYNC_INVALIDATE_DELAY_MS = 3000

/** Duration (ms) to show copy-success feedback before resetting. */
export const COPY_FEEDBACK_MS = 2000

/** Stale time for data that changes infrequently (keys, usage, models). */
export const STALE_TIME_SLOW = 59_000

/** Stale time for near-realtime data (dashboard stats, capacity). */
export const STALE_TIME_FAST = 29_000

/** Refetch interval for near-realtime data. */
export const REFETCH_INTERVAL_FAST = 30_000

/** Refetch interval for data that changes infrequently (keys, usage, models). */
export const REFETCH_INTERVAL_SLOW = 60_000

/** Stale time for live-polled data (queue depth). */
export const STALE_TIME_LIVE = 2_000

/** Refetch interval for live-polled data (queue depth — 3s). */
export const REFETCH_INTERVAL_LIVE = 3_000

/** Refetch interval for historical data (power history, metric history). */
export const REFETCH_INTERVAL_HISTORY = 5 * 60_000

/**
 * Add random jitter to a refetch interval to prevent synchronized polling storms
 * when many browser tabs open at the same time.
 *
 * Returns base + U[0, maxJitter) ms — always ≥ base, never shrinks the interval.
 */
export function withJitter(base: number, maxJitter = 5_000): number {
  return base + Math.floor(Math.random() * maxJitter)
}

/** Stale time for long-window historical data (60-day power / metrics history).
 *  Data is refetched in the background every REFETCH_INTERVAL_HISTORY, so it
 *  stays fresh — but navigating away and back within 30 min skips the on-mount
 *  fetch and uses the cached data immediately. */
export const STALE_TIME_HISTORY = 30 * 60_000

// ── Metric thresholds — SSOT for colour-coded health indicators ─────────────

/** GPU temperature (°C): critical ≥ this value. */
export const GPU_TEMP_CRITICAL = 85
/** GPU temperature (°C): warning ≥ this value. */
export const GPU_TEMP_WARNING = 70

/** CPU / Memory usage (%): critical ≥ this value. */
export const RESOURCE_CRITICAL = 90
/** CPU / Memory usage (%): warning ≥ this value. */
export const RESOURCE_WARNING = 75

/** Success rate (%): values ≥ this are "good". */
export const SUCCESS_RATE_GOOD = 90
/** Success rate (%): values ≥ this (but < GOOD) are "warning". */
export const SUCCESS_RATE_WARNING = 70

/** Chat message role → Tailwind badge class. */
export const ROLE_STYLES: Record<string, string> = {
  system:    'vds-bg-muted vds-text-dim vds-border-subtle',
  user:      'vds-bg-info/10 vds-text-info vds-border-info/30',
  assistant: 'vds-bg-success/10 vds-text-success vds-border-success/30',
  tool:      'vds-bg-warning/10 vds-text-warning vds-border-warning/30',
}

// ── Provider status O(1) lookups ──────────────────────────────────────────────

/** Provider status → dot indicator class (solid bg). */
export const PROVIDER_STATUS_DOT: Record<string, string> = {
  online:   'vds-h-2 vds-w-2 vds-rounded-full vds-bg-success vds-flex-shrink-0',
  degraded: 'vds-h-2 vds-w-2 vds-rounded-full vds-bg-warning vds-flex-shrink-0',
  offline:  'vds-h-2 vds-w-2 vds-rounded-full vds-bg-error vds-flex-shrink-0',
}

/** Provider status → dot indicator class (muted offline variant). */
export const PROVIDER_STATUS_DOT_ALT: Record<string, string> = {
  online:   'vds-h-2 vds-w-2 vds-rounded-full vds-bg-success vds-flex-shrink-0',
  degraded: 'vds-h-2 vds-w-2 vds-rounded-full vds-bg-warning vds-flex-shrink-0',
  offline:  'vds-h-2 vds-w-2 vds-rounded-full vds-bg-neutral/40 vds-flex-shrink-0',
}

/** Provider status → badge class. */
export const PROVIDER_STATUS_BADGE: Record<string, string> = {
  online:   'vds-text-success vds-border-success/40 vds-text-[10px]',
  degraded: 'vds-text-warning vds-border-warning/40 vds-text-[10px]',
  offline:  'vds-text-error vds-border-error/40 vds-text-[10px]',
}

/** Provider status → text colour class. */
export const PROVIDER_STATUS_TEXT: Record<string, string> = {
  online:   'vds-text-success',
  degraded: 'vds-text-warning',
  offline:  'vds-text-dim',
}

/** Provider status → i18n key. */
export const PROVIDER_STATUS_I18N: Record<string, string> = {
  online:   'common.online',
  degraded: 'common.degraded',
  offline:  'common.offline',
}

/** Service health status → dot indicator class. */
export const SERVICE_STATUS_DOT: Record<string, string> = {
  ok:          'vds-h-2 vds-w-2 vds-rounded-full vds-bg-success vds-flex-shrink-0',
  degraded:    'vds-h-2 vds-w-2 vds-rounded-full vds-bg-warning vds-flex-shrink-0',
  unavailable: 'vds-h-2 vds-w-2 vds-rounded-full vds-bg-error vds-flex-shrink-0',
}

/** Service health status → text colour class. */
export const SERVICE_STATUS_TEXT: Record<string, string> = {
  ok:          'vds-text-success',
  degraded:    'vds-text-warning',
  unavailable: 'vds-text-error',
}

/** Job source → Tailwind badge class. SSOT for source origin badges. */
export const SOURCE_STYLES: Record<string, string> = {
  test:     'vds-bg-warning/15 vds-text-warning',
  analyzer: 'vds-bg-hover/15 vds-text-primary',
  api:      'vds-bg-primary/10 vds-text-primary',
}

/** Job status → Tailwind class mapping. SSOT for all status badges. */
export const STATUS_STYLES: Record<string, string> = {
  completed: 'vds-bg-success/15 vds-text-success vds-border-success/30',
  failed:    'vds-bg-error/15 vds-text-error vds-border-error/30',
  cancelled: 'vds-bg-cancelled/15 vds-text-dim vds-border-neutral/30',
  pending:   'vds-bg-warning/15 vds-text-warning vds-border-warning/30',
  running:   'vds-bg-info/15 vds-text-info vds-border-info/30',
}
