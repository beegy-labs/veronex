'use client'

import { useQuery } from '@tanstack/react-query'
import { serverMetricsQuery } from '@/lib/queries/servers'
import { Thermometer, Zap, MemoryStick, WifiOff, RefreshCw, Cpu } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { fmtMb, fmtTemp, fmtPower, fmtPct } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import {
  GPU_TEMP_CRITICAL, GPU_TEMP_WARNING,
  RESOURCE_CRITICAL, RESOURCE_WARNING,
} from '@/lib/constants'

// ── Full-width metrics cell (Servers page) ────────────────────────────────────

export function ServerMetricsCell({ serverId }: { serverId: string }) {
  const { t } = useTranslation()
  const { data, isLoading, isError, refetch, isFetching } = useQuery(serverMetricsQuery(serverId))

  if (isLoading) {
    return <span className="vds-text-xs vds-text-dim vds-animate-pulse">{t('common.loading')}</span>
  }

  if (isError || !data || !data.scrape_ok) {
    return (
      <div className="vds-flex vds-items-center vds-gap-2">
        <Badge variant="outline" className="vds-bg-error/10 vds-text-error vds-border-error/30 vds-text-xs vds-font-500">
          <WifiOff className="vds-h-3 vds-w-3 vds-mr-1.5" />{t('providers.servers.unreachable')}
        </Badge>
        <Button variant="ghost" size="icon" className="vds-h-6 vds-w-6 vds-text-dim vds-hover:text-primary"
          aria-label={t('common.retry')} onClick={() => refetch()} disabled={isFetching} title={t('common.retry')}>
          <RefreshCw className={isFetching ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
        </Button>
      </div>
    )
  }

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const memPct = calcPercentage(memUsed, data.mem_total_mb)
  const cpuPct = data.cpu_usage_pct != null ? Math.round(data.cpu_usage_pct) : null

  return (
    <div className="vds-space-y-1 vds-text-xs">
      {/* MEM */}
      <div className="vds-flex vds-items-center vds-gap-2">
        <span className="vds-w-6 vds-text-[10px] vds-font-600 vds-text-dim/70 vds-uppercase vds-tracking-wide vds-flex-shrink-0">MEM</span>
        <span className="vds-text-bright vds-font-mono vds-tabular-nums">
          {fmtMb(memUsed)}<span className="vds-text-dim/70"> / {fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`vds-ml-auto vds-font-600 vds-tabular-nums ${memPct >= RESOURCE_CRITICAL ? 'vds-text-error' : memPct >= RESOURCE_WARNING ? 'vds-text-warning' : 'vds-text-dim'}`}>
          {memPct}%
        </span>
      </div>

      {/* CPU */}
      {data.cpu_logical > 0 && (
        <div className="vds-flex vds-items-center vds-gap-2">
          <span className="vds-w-6 vds-text-[10px] vds-font-600 vds-text-dim/70 vds-uppercase vds-tracking-wide vds-flex-shrink-0">CPU</span>
          <span className="vds-text-dim vds-tabular-nums">
            {data.cpu_physical != null
              ? <>{data.cpu_physical}<span className="vds-text-dim/60">c</span> / {data.cpu_logical}<span className="vds-text-dim/60">t</span></>
              : <>{data.cpu_logical}<span className="vds-text-dim/60">t</span></>}
          </span>
          {cpuPct != null && (
            <span className={`vds-ml-auto vds-font-600 vds-tabular-nums ${cpuPct >= RESOURCE_CRITICAL ? 'vds-text-error' : cpuPct >= RESOURCE_WARNING ? 'vds-text-warning' : 'vds-text-dim'}`}>
              {cpuPct}%
            </span>
          )}
        </div>
      )}

      {/* GPU rows */}
      {data.gpus.map((gpu) => {
        const gpuT = gpu.temp_junction_c ?? gpu.temp_c
        return (
        <div key={gpu.card} className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap">
          <span className="vds-w-6 vds-text-[10px] vds-font-600 vds-text-accent-gpu vds-uppercase vds-tracking-wide vds-flex-shrink-0">GPU</span>
          <span className="vds-text-dim vds-font-mono">{gpu.card}</span>
          {gpuT != null && (
              <span className={`vds-flex vds-items-center vds-gap-0.5 vds-tabular-nums ${gpuT >= GPU_TEMP_CRITICAL ? 'vds-text-error vds-font-bold' : 'vds-text-dim'}`}>
                <Thermometer className="vds-h-3 vds-w-3" />{fmtTemp(gpuT)}
              </span>
          )}
          {gpu.power_w != null && (
            <span className="vds-flex vds-items-center vds-gap-0.5 vds-text-dim vds-tabular-nums">
              <Zap className="vds-h-3 vds-w-3 vds-text-accent-power" />{fmtPower(gpu.power_w)}
            </span>
          )}
          {gpu.vram_total_mb != null && (
            <span className="vds-flex vds-items-center vds-gap-0.5 vds-text-dim vds-tabular-nums">
              <MemoryStick className="vds-h-3 vds-w-3" />{fmtMb(gpu.vram_used_mb ?? 0)}/{fmtMb(gpu.vram_total_mb)}
            </span>
          )}
          {gpu.busy_pct != null && (
            <span className="vds-text-dim vds-tabular-nums">{fmtPct(gpu.busy_pct)}</span>
          )}
        </div>
        )
      })}
    </div>
  )
}

// ── Compact inline metrics (Ollama Providers tab) ─────────────────────────────

export function ServerMetricsCompact({
  serverId,
  gpuIndex,
}: {
  serverId: string
  gpuIndex: number | null
}) {
  const { t } = useTranslation()
  const { data, isError } = useQuery(serverMetricsQuery(serverId))

  if (isError || (data && !data.scrape_ok)) {
    return <span className="vds-text-[10px] vds-text-error vds-italic">{t('providers.servers.unreachable')}</span>
  }
  if (!data) return null

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const memPct = calcPercentage(memUsed, data.mem_total_mb)
  const cpuPct = data.cpu_usage_pct != null ? Math.round(data.cpu_usage_pct) : null
  const gpu = data.gpus[gpuIndex ?? 0] ?? null
  const gpuTemp = gpu?.temp_junction_c ?? gpu?.temp_c ?? null
  const tempCls = gpuTemp != null && gpuTemp >= GPU_TEMP_CRITICAL
    ? 'vds-text-error'
    : gpuTemp != null && gpuTemp >= GPU_TEMP_WARNING
    ? 'vds-text-warning'
    : 'vds-text-dim'

  return (
    <div className="vds-mt-1.5 vds-pt-1.5 vds-border-t-1 vds-border-subtle/40 vds-flex vds-flex-wrap vds-items-center vds-gap-x-2.5 vds-gap-y-0.5">
      <span className="vds-flex vds-items-center vds-gap-1">
        <span className="vds-text-[10px] vds-font-600 vds-text-dim/60 vds-uppercase">MEM</span>
        <span className="vds-tabular-nums vds-font-mono vds-text-2xs vds-text-dim">
          {fmtMb(memUsed)}<span className="vds-text-dim/40">/{fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`vds-text-[10px] vds-tabular-nums ${memPct >= RESOURCE_CRITICAL ? 'vds-text-error' : memPct >= RESOURCE_WARNING ? 'vds-text-warning' : 'vds-text-dim/70'}`}>
          {memPct}%
        </span>
      </span>
      {cpuPct != null && (
        <span className="vds-flex vds-items-center vds-gap-0.5 vds-text-2xs vds-tabular-nums vds-text-dim">
          <Cpu className="vds-h-3 vds-w-3 vds-flex-shrink-0" />
          <span className={cpuPct >= RESOURCE_CRITICAL ? 'vds-text-error vds-font-700' : cpuPct >= RESOURCE_WARNING ? 'vds-text-warning' : ''}>
            {cpuPct}%
          </span>
        </span>
      )}
      {gpuTemp != null && (
        <span className={`vds-flex vds-items-center vds-gap-0.5 vds-text-2xs vds-tabular-nums ${tempCls}`}>
          <Thermometer className="vds-h-3 vds-w-3 vds-flex-shrink-0" />{fmtTemp(gpuTemp)}
        </span>
      )}
      {gpu?.power_w != null && (
        <span className="vds-flex vds-items-center vds-gap-0.5 vds-text-2xs vds-tabular-nums vds-text-dim">
          <Zap className="vds-h-3 vds-w-3 vds-flex-shrink-0 vds-text-accent-power" />{fmtPower(gpu.power_w)}
        </span>
      )}
    </div>
  )
}
