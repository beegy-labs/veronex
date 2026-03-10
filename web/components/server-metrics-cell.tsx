'use client'

import { useQuery } from '@tanstack/react-query'
import { serverMetricsQuery } from '@/lib/queries/servers'
import { Thermometer, Zap, MemoryStick, WifiOff, RefreshCw, Cpu } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { fmtMb } from '@/lib/chart-theme'
import {
  GPU_TEMP_CRITICAL, GPU_TEMP_WARNING,
  RESOURCE_CRITICAL, RESOURCE_WARNING,
} from '@/lib/constants'

export { fmtMb } from '@/lib/chart-theme'

// ── Full-width metrics cell (Servers page) ────────────────────────────────────

export function ServerMetricsCell({ serverId }: { serverId: string }) {
  const { t } = useTranslation()
  const { data, isLoading, isError, refetch, isFetching } = useQuery(serverMetricsQuery(serverId))

  if (isLoading) {
    return <span className="text-xs text-muted-foreground animate-pulse">{t('common.loading')}</span>
  }

  if (isError || !data || !data.scrape_ok) {
    return (
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="bg-status-error/10 text-status-error-fg border-status-error/30 text-xs font-medium">
          <WifiOff className="h-3 w-3 mr-1.5" />{t('providers.servers.unreachable')}
        </Badge>
        <Button variant="ghost" size="icon" className="h-6 w-6 text-muted-foreground hover:text-foreground"
          onClick={() => refetch()} disabled={isFetching} title={t('common.retry')}>
          <RefreshCw className={isFetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
        </Button>
      </div>
    )
  }

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const memPct = data.mem_total_mb > 0 ? Math.round((memUsed / data.mem_total_mb) * 100) : 0
  const cpuPct = data.cpu_usage_pct != null ? Math.round(data.cpu_usage_pct) : null

  return (
    <div className="space-y-1 text-xs">
      {/* MEM */}
      <div className="flex items-center gap-2">
        <span className="w-6 text-[10px] font-semibold text-muted-foreground/70 uppercase tracking-wide shrink-0">MEM</span>
        <span className="text-text-bright font-mono tabular-nums">
          {fmtMb(memUsed)}<span className="text-muted-foreground/70"> / {fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`ml-auto font-semibold tabular-nums ${memPct >= RESOURCE_CRITICAL ? 'text-status-error-fg' : memPct >= RESOURCE_WARNING ? 'text-status-warning-fg' : 'text-muted-foreground'}`}>
          {memPct}%
        </span>
      </div>

      {/* CPU */}
      {data.cpu_logical > 0 && (
        <div className="flex items-center gap-2">
          <span className="w-6 text-[10px] font-semibold text-muted-foreground/70 uppercase tracking-wide shrink-0">CPU</span>
          <span className="text-text-dim tabular-nums">
            {data.cpu_physical != null
              ? <>{data.cpu_physical}<span className="text-muted-foreground/60">c</span> / {data.cpu_logical}<span className="text-muted-foreground/60">t</span></>
              : <>{data.cpu_logical}<span className="text-muted-foreground/60">t</span></>}
          </span>
          {cpuPct != null && (
            <span className={`ml-auto font-semibold tabular-nums ${cpuPct >= RESOURCE_CRITICAL ? 'text-status-error-fg' : cpuPct >= RESOURCE_WARNING ? 'text-status-warning-fg' : 'text-muted-foreground'}`}>
              {cpuPct}%
            </span>
          )}
        </div>
      )}

      {/* GPU rows */}
      {data.gpus.map((gpu) => (
        <div key={gpu.card} className="flex items-center gap-2 flex-wrap">
          <span className="w-6 text-[10px] font-semibold text-accent-gpu uppercase tracking-wide shrink-0">GPU</span>
          <span className="text-text-dim font-mono">{gpu.card}</span>
          {(gpu.temp_junction_c ?? gpu.temp_c) != null && (() => {
            const t = gpu.temp_junction_c ?? gpu.temp_c!
            return (
              <span className={`flex items-center gap-0.5 tabular-nums ${t >= GPU_TEMP_CRITICAL ? 'text-status-error-fg font-bold' : 'text-text-dim'}`}>
                <Thermometer className="h-3 w-3" />{t.toFixed(0)}°C
              </span>
            )
          })()}
          {gpu.power_w != null && (
            <span className="flex items-center gap-0.5 text-text-dim tabular-nums">
              <Zap className="h-3 w-3 text-accent-power" />{gpu.power_w.toFixed(0)}W
            </span>
          )}
          {gpu.vram_total_mb != null && (
            <span className="flex items-center gap-0.5 text-muted-foreground tabular-nums">
              <MemoryStick className="h-3 w-3" />{fmtMb(gpu.vram_used_mb ?? 0)}/{fmtMb(gpu.vram_total_mb)}
            </span>
          )}
          {gpu.busy_pct != null && (
            <span className="text-muted-foreground tabular-nums">{gpu.busy_pct.toFixed(0)}%</span>
          )}
        </div>
      ))}
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
    return <span className="text-[10px] text-status-error-fg italic">{t('providers.servers.unreachable')}</span>
  }
  if (!data) return null

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const memPct = data.mem_total_mb > 0 ? Math.round((memUsed / data.mem_total_mb) * 100) : 0
  const cpuPct = data.cpu_usage_pct != null ? Math.round(data.cpu_usage_pct) : null
  const gpu = data.gpus[gpuIndex ?? 0] ?? null
  const gpuTemp = gpu?.temp_junction_c ?? gpu?.temp_c ?? null
  const tempCls = gpuTemp != null && gpuTemp >= GPU_TEMP_CRITICAL
    ? 'text-status-error-fg'
    : gpuTemp != null && gpuTemp >= GPU_TEMP_WARNING
    ? 'text-status-warn-fg'
    : 'text-muted-foreground'

  return (
    <div className="mt-1.5 pt-1.5 border-t border-border/40 flex flex-wrap items-center gap-x-2.5 gap-y-0.5">
      <span className="flex items-center gap-1">
        <span className="text-[10px] font-semibold text-muted-foreground/60 uppercase">MEM</span>
        <span className="tabular-nums font-mono text-[11px] text-text-dim">
          {fmtMb(memUsed)}<span className="text-muted-foreground/40">/{fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`text-[10px] tabular-nums ${memPct >= RESOURCE_CRITICAL ? 'text-status-error-fg' : memPct >= RESOURCE_WARNING ? 'text-status-warning-fg' : 'text-muted-foreground/70'}`}>
          {memPct}%
        </span>
      </span>
      {cpuPct != null && (
        <span className="flex items-center gap-0.5 text-[11px] tabular-nums text-muted-foreground">
          <Cpu className="h-3 w-3 shrink-0" />
          <span className={cpuPct >= RESOURCE_CRITICAL ? 'text-status-error-fg font-bold' : cpuPct >= RESOURCE_WARNING ? 'text-status-warning-fg' : ''}>
            {cpuPct}%
          </span>
        </span>
      )}
      {gpuTemp != null && (
        <span className={`flex items-center gap-0.5 text-[11px] tabular-nums ${tempCls}`}>
          <Thermometer className="h-3 w-3 shrink-0" />{gpuTemp.toFixed(0)}°C
        </span>
      )}
      {gpu?.power_w != null && (
        <span className="flex items-center gap-0.5 text-[11px] tabular-nums text-muted-foreground">
          <Zap className="h-3 w-3 shrink-0 text-accent-power" />{gpu.power_w.toFixed(0)}W
        </span>
      )}
    </div>
  )
}
