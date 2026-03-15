'use client'

import { useState } from 'react'
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

  const chartData = (data ?? []).map((p) => ({
    ts: fmtTimeHHMM(p.ts),
    memUsedPct: p.mem_total_mb > 0
      ? calcPercentage(p.mem_total_mb - p.mem_avail_mb, p.mem_total_mb) : 0,
    gpuTemp: p.gpu_temp_junction_c ?? p.gpu_temp_c ?? undefined,
    gpuPower: p.gpu_power_w !== null ? Math.round((p.gpu_power_w ?? 0) * 10) / 10 : undefined,
  }))

  const hasGpu = (data ?? []).some((p) => p.gpu_temp_junction_c !== null || p.gpu_temp_c !== null || p.gpu_power_w !== null)

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <BarChart2 className="h-4 w-4 text-accent-gpu" />
            {server.name}
            <span className="text-muted-foreground font-normal text-sm">— {t('providers.clickhouseHistory')}</span>
          </DialogTitle>
        </DialogHeader>

        <div className="flex items-center gap-2 border-b border-border pb-3">
          <div className="flex items-center gap-1 bg-muted rounded-md p-0.5">
            {HIST_HOUR_OPTIONS.map((h) => (
              <Button key={h} size="sm" variant={hours === h ? 'default' : 'ghost'}
                onClick={() => setHours(h as typeof hours)}
                className="h-6 px-3 text-xs rounded">
                {h}h
              </Button>
            ))}
          </div>
          <Button size="sm" variant="ghost" onClick={() => refetch()} disabled={isFetching}
            className="h-7 px-2 ml-auto gap-1.5 text-xs text-muted-foreground hover:text-foreground">
            <RefreshCw className={isFetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
            {t('common.sync')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}
        {isError && (
          <p className="text-sm text-destructive py-2">{t('providers.checkOtel')}</p>
        )}
        {data && data.length === 0 && (
          <p className="text-sm text-muted-foreground py-6 text-center">
            {t('providers.noClickhouseData', { hours })}
            <br />
            <span className="text-xs opacity-60">{t('providers.checkOtel')}</span>
          </p>
        )}

        {data && data.length > 0 && (
          <div className="space-y-5">
            <div>
              <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('providers.memUsedPct')}</p>
              <ResponsiveContainer width="100%" height={110}>
                <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                  <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                  <YAxis domain={[0, 100]} tick={AXIS_TICK_SM} unit="%" />
                  <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}%`, 'Mem Used'] as [string, string]} />
                  <Line type="monotone" dataKey="memUsedPct" stroke="var(--theme-status-info)" dot={false} strokeWidth={2} />
                </LineChart>
              </ResponsiveContainer>
            </div>
            {hasGpu && (
              <>
                <div>
                  <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('providers.gpuTempC')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                      <YAxis tick={AXIS_TICK_SM} unit="°C" />
                      <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}°C`, 'GPU Temp'] as [string, string]} />
                      <Line type="monotone" dataKey="gpuTemp" stroke="var(--theme-status-error)" dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
                <div>
                  <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('providers.gpuPowerW')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={AXIS_TICK_SM} interval="preserveStartEnd" />
                      <YAxis tick={AXIS_TICK_SM} unit="W" />
                      <Tooltip contentStyle={TOOLTIP_STYLE} formatter={(v) => [`${Number(v)}W`, 'GPU Power'] as [string, string]} />
                      <Line type="monotone" dataKey="gpuPower" stroke="var(--theme-accent-power)" dot={false} strokeWidth={2} connectNulls />
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
