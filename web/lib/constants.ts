/** Single source of truth for the Veronex backend URL. */
export const BASE_API_URL =
  process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

/** Provider type identifiers — single source of truth. */
export const PROVIDER_OLLAMA = 'ollama' as const
export const PROVIDER_GEMINI = 'gemini' as const

/** Provider type → Tailwind badge class. */
export const PROVIDER_BADGE: Record<string, string> = {
  ollama: 'bg-primary/10 text-primary border-primary/30',
  gemini: 'bg-status-info/10 text-status-info-fg border-status-info/30',
}

/** Provider type → CSS custom-property chart colour. */
export const PROVIDER_COLORS: Record<string, string> = {
  ollama: 'var(--theme-primary)',
  gemini: 'var(--theme-status-info)',
}

/** Finish reason → chart colour. */
export const FINISH_COLORS: Record<string, string> = {
  stop:      'var(--theme-status-success)',
  length:    'var(--theme-status-warning)',
  error:     'var(--theme-status-error)',
  cancelled: 'var(--theme-text-secondary)',
}

/** Finish reason → Tailwind badge class. */
export const FINISH_BG: Record<string, string> = {
  stop:      'bg-status-success/15 text-status-success-fg border-status-success/30',
  length:    'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  error:     'bg-status-error/15 text-status-error-fg border-status-error/30',
  cancelled: 'bg-muted text-muted-foreground border-border',
}

/** Stale time for data that changes infrequently (keys, usage, models). */
export const STALE_TIME_SLOW = 59_000

/** Stale time for near-realtime data (dashboard stats, capacity). */
export const STALE_TIME_FAST = 29_000

/** Refetch interval for near-realtime data. */
export const REFETCH_INTERVAL_FAST = 30_000

/** Chat message role → Tailwind badge class. */
export const ROLE_STYLES: Record<string, string> = {
  system:    'bg-muted text-muted-foreground border-border',
  user:      'bg-status-info/10 text-status-info-fg border-status-info/30',
  assistant: 'bg-status-success/10 text-status-success-fg border-status-success/30',
  tool:      'bg-status-warning/10 text-status-warning-fg border-status-warning/30',
}

// ── Provider status O(1) lookups ──────────────────────────────────────────────

/** Provider status → dot indicator class (solid bg). */
export const PROVIDER_STATUS_DOT: Record<string, string> = {
  online:   'h-2 w-2 rounded-full bg-status-success shrink-0',
  degraded: 'h-2 w-2 rounded-full bg-status-warn shrink-0',
  offline:  'h-2 w-2 rounded-full bg-status-error shrink-0',
}

/** Provider status → dot indicator class (muted offline variant). */
export const PROVIDER_STATUS_DOT_ALT: Record<string, string> = {
  online:   'h-2 w-2 rounded-full bg-status-success shrink-0',
  degraded: 'h-2 w-2 rounded-full bg-status-warn shrink-0',
  offline:  'h-2 w-2 rounded-full bg-muted-foreground/40 shrink-0',
}

/** Provider status → badge class. */
export const PROVIDER_STATUS_BADGE: Record<string, string> = {
  online:   'text-status-success-fg border-status-success/40 text-[10px]',
  degraded: 'text-status-warn-fg border-status-warn/40 text-[10px]',
  offline:  'text-status-error-fg border-status-error/40 text-[10px]',
}

/** Provider status → text colour class. */
export const PROVIDER_STATUS_TEXT: Record<string, string> = {
  online:   'text-status-success-fg',
  degraded: 'text-status-warn-fg',
  offline:  'text-muted-foreground',
}

/** Provider status → i18n key. */
export const PROVIDER_STATUS_I18N: Record<string, string> = {
  online:   'common.online',
  degraded: 'common.degraded',
  offline:  'common.offline',
}

/** Job status → Tailwind class mapping. SSOT for all status badges. */
export const STATUS_STYLES: Record<string, string> = {
  completed: 'bg-status-success/15 text-status-success-fg border-status-success/30',
  failed:    'bg-status-error/15 text-status-error-fg border-status-error/30',
  cancelled: 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30',
  pending:   'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  running:   'bg-status-info/15 text-status-info-fg border-status-info/30',
}
