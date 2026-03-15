import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

import { PROVIDER_OLLAMA, PROVIDER_GEMINI, SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING } from './constants'
import type { Provider } from './types'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

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
  if (rate == null) return 'text-muted-foreground'
  if (rate >= SUCCESS_RATE_GOOD) return 'bg-status-success/15 text-status-success-fg'
  if (rate >= SUCCESS_RATE_WARNING) return 'bg-status-warning/15 text-status-warning-fg'
  return 'bg-status-error/15 text-status-error-fg'
}
