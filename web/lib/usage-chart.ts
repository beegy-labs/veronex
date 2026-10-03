import type { HourlyUsage } from './types'
import { fmtHourLabel } from './date'

/** Shared hourly usage projection for the usage page and key detail modal. */
export function toUsageChartData(hourly: HourlyUsage[] | undefined, tz: string) {
  return (
    (hourly ?? []).map((h) => ({
      hour:     fmtHourLabel(h.hour, tz),
      tokens:   h.total_tokens,
      prompt:   h.prompt_tokens,
      compl:    h.completion_tokens,
      requests: h.request_count,
      success:  h.success_count,
      errors:   h.error_count,
    }))
  )
}
