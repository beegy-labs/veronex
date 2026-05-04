import { PROVIDER_OLLAMA, PROVIDER_GEMINI, SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING } from './constants'
import type { Provider } from './types'

export { cn } from './vds-merge'

// ── Provider filtering (SSOT) ─────────────────────────────────────────────

export const getOllamaProviders = (providers: Provider[] | undefined) =>
  providers?.filter(p => p.provider_type === PROVIDER_OLLAMA) ?? []

export const getGeminiProviders = (providers: Provider[] | undefined) =>
  providers?.filter(p => p.provider_type === PROVIDER_GEMINI) ?? []

// ── Status counting ───────────────────────────────────────────────────────

export const countByStatus = (items: { status: string }[]): Record<string, number> =>
  items.reduce<Record<string, number>>((acc, item) => {
    acc[item.status] = (acc[item.status] ?? 0) + 1
    return acc
  }, {})

// ── Percentage ────────────────────────────────────────────────────────────

export const calcPercentage = (numerator: number, denominator: number): number =>
  denominator > 0 ? Math.round((numerator / denominator) * 100) : 0

// ── Success rate styling ──────────────────────────────────────────────────

export function successRateCls(rate: number | undefined): string {
  if (rate == null) return 'vds-text-dim'
  if (rate >= SUCCESS_RATE_GOOD) return 'vds-bg-success-bg vds-text-success'
  if (rate >= SUCCESS_RATE_WARNING) return 'vds-bg-warning-bg vds-text-warning'
  return 'vds-bg-error-bg vds-text-error'
}
