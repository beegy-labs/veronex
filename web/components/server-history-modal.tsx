'use client'

import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import type { GpuServer } from '@/lib/types'
import { serverMetricsHistoryQuery } from '@/lib/queries'
import { BarChart2, RefreshCw } from 'lucide-react'
import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'
import { Button } from '@/components/ui/button'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'
import { TOOLTIP_STYLE, AXIS_TICK_SM, fmtTimeHHMM } from '@/lib/chart-theme'
import { tokens } from '@/lib/design-tokens'
import { calcPercentage } from '@/lib/utils'

const HIST_HOUR_OPTIONS = [1, 3, 6, 24] as const

export function ServerHistoryModal({
  server,
  onClose,
}: {
  server: GpuServer
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [hours, setHours] = useState<1 | 3 | 6 | 24>(1)

  const { data, isLoading, isError, refetch, isFetching } = useQuery(serverMetricsHistoryQuery(server.id, hours))

  const chartData = useMemo(() =>
    (data ?? []).map((p) => ({
      ts: fmtTimeHHMM(p.ts),
      memUsedPct: p.mem_total_mb > 0
        ? calcPercentage(p.mem_total_mb - p.mem_avail_mb, p.mem_total_mb) : 0,
      gpuTemp: p.gpu_temp_junction_c ?? p.gpu_temp_c ?? undefined,
      gpuPower: p.gpu_power_w !== null ? Math.round((p.gpu_power_w ?? 0) * 10) / 10 : undefined,
    })),
    [data],
  )

  const hasGpu = (data ?? []).some((p) => p.gpu_temp_junction_c !== null || p.gpu_temp_c !== null || p.gpu_power_w !== null)

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-[95vw] vds-sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <BarChart2 className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
            {server.name}
            <span className="vds-text-dim vds-font-400 vds-text-sm">— {t('providers.clickhouseHistory')}</span>
          </DialogTitle>
        </DialogHeader>

        <div className="vds-flex vds-items-center vds-gap-2 vds-border-b-1 vds-border-subtle vds-pb-3">
          <div className="vds-flex vds-items-center vds-gap-1 vds-bg-muted vds-rounded-md vds-p-0.5">
            {HIST_HOUR_OPTIONS.map((h) => (
              <Button key={h} size="sm" variant={hours === h ? 'default' : 'ghost'}
                onClick={() => setHours(h as typeof hours)}
                className="vds-h-6 vds-px-3 vds-text-xs vds-rounded">
                {h}h
              </Button>
            ))}
          </div>
          <Button size="sm" variant="ghost" onClick={() => refetch()} disabled={isFetching}
            className="vds-h-7 vds-px-2 vds-ml-auto vds-gap-1.5 vds-text-xs vds-text-dim vds-hover:text-primary">
            <RefreshCw className={isFetching ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
            {t('common.sync')}
          </Button>
        </div>

        {isLoading && (
          <div className="vds-flex vds-h-32 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('common.loading')}
          </div>
        )}
        {isError && (
          <p className="vds-text-sm vds-text-destructive vds-py-2">{t('providers.checkOtel')}</p>
        )}
        {data && data.length === 0 && (
          <p className="vds-text-sm vds-text-dim vds-py-6 vds-text-center">
            {t('providers.noClickhouseData', { hours })}
            <br />
            <span className="vds-text-xs vds-opacity-60">{t('providers.checkOtel')}</span>
          </p>
        )}

        {data && data.length > 0 && (
          <div className="vds-space-y-5">
            <div>
              <p className="vds-text-xs vds-font-600 vds-text-dim vds-uppercase vds-tracking-wide vds-mb-2">{t('providers.memUsedPct')}</p>
              <ResponsiveContainer width="100%" height={110}>
                <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                  <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                  <YAxis domain={[0, 100]} tick={AXIS_TICK_SM} unit="%" />
                  <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}%`, t('providers.memUsedPct')] as [string, string]} />
                  <Line type="monotone" dataKey="memUsedPct" stroke={tokens.status.info} dot={false} strokeWidth={2} />
                </LineChart>
              </ResponsiveContainer>
            </div>
            {hasGpu && (
              <>
                <div>
                  <p className="vds-text-xs vds-font-600 vds-text-dim vds-uppercase vds-tracking-wide vds-mb-2">{t('providers.gpuTempC')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                      <YAxis tick={AXIS_TICK_SM} unit="°C" />
                      <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}°C`, t('providers.gpuTempC')] as [string, string]} />
                      <Line type="monotone" dataKey="gpuTemp" stroke={tokens.status.error} dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
                <div>
                  <p className="vds-text-xs vds-font-600 vds-text-dim vds-uppercase vds-tracking-wide vds-mb-2">{t('providers.gpuPowerW')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                      <YAxis tick={AXIS_TICK_SM} unit="W" />
                      <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}W`, t('providers.gpuPowerW')] as [string, string]} />
                      <Line type="monotone" dataKey="gpuPower" stroke={tokens.accent.power} dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
