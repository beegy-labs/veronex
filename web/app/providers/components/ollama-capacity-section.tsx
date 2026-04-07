'use client'

import React, { useState, useRef, useMemo, useEffect, useCallback, memo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { PatchSyncSettings, ProviderVramInfo } from '@/lib/types'
import { capacityQuery, capacityClusterQuery, syncSettingsQuery } from '@/lib/queries'
import { Activity, AlertTriangle, ChevronDown, ChevronRight, Layers, RefreshCw, Search, Server } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent } from '@/components/ui/card'
import {
  Select, SelectContent, SelectGroup, SelectItem, SelectLabel,
  SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { useTranslation } from '@/i18n'
import { fmtMbShort, fmtTemp } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { RESOURCE_CRITICAL, RESOURCE_WARNING, SYNC_INVALIDATE_DELAY_MS } from '@/lib/constants'
import { useLabSettings } from '@/components/lab-settings-provider'
import { ProgressBar } from '@/components/progress-bar'

export const ThermalBadge = memo(function ThermalBadge({ state }: { state: 'normal' | 'soft' | 'hard' }) {
  const { t } = useTranslation()
  if (state === 'hard') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-error/15 text-status-error-fg border border-status-error/30">
      <AlertTriangle className="h-2.5 w-2.5" />{t('providers.capacity.thermal.hard')}
    </span>
  )
  if (state === 'soft') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-warning/15 text-status-warning-fg border border-status-warning/30">
      <AlertTriangle className="h-2.5 w-2.5" />{t('providers.capacity.thermal.soft')}
    </span>
  )
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-success/10 text-status-success-fg border border-status-success/30">
      <span className="h-1.5 w-1.5 rounded-full bg-status-success" />{t('providers.capacity.thermal.normal')}
    </span>
  )
})

export const VramBar = memo(function VramBar({ used, total }: { used: number; total: number }) {
  const { t } = useTranslation()
  if (total === 0) return <span className="text-xs text-muted-foreground italic">{t('common.na')}</span>
  const pct = Math.min(100, calcPercentage(used, total))
  const color = pct > RESOURCE_CRITICAL ? 'bg-status-error' : pct > RESOURCE_WARNING ? 'bg-status-warning' : 'bg-status-success'
  return (
    <div className="flex items-center gap-2 min-w-24">
      <ProgressBar pct={pct} height="h-1.5" colorClass={color} trackClass="bg-muted/60" className="flex-1" />
      <span className="text-[11px] text-muted-foreground tabular-nums shrink-0">{pct}%</span>
    </div>
  )
})

// ── Memoized provider row — skips re-render when other providers toggle/update ─

const ProviderRow = memo(function ProviderRow({
  provider,
  isCollapsed,
  onToggle,
}: {
  provider: ProviderVramInfo
  isCollapsed: boolean
  onToggle: (id: string) => void
}) {
  const { t } = useTranslation()
  return (
    <React.Fragment>
      <TableRow
        className="border-t border-border bg-muted/40 cursor-pointer hover:bg-muted/60 transition-colors"
        onClick={() => onToggle(provider.provider_id)}
      >
        <TableCell colSpan={4} className="px-3 py-1.5">
          <div className="flex items-center gap-2 min-w-0">
            {isCollapsed
              ? <ChevronRight className="h-3 w-3 text-muted-foreground/50 shrink-0" />
              : <ChevronDown className="h-3 w-3 text-muted-foreground/50 shrink-0" />
            }
            <Server className="h-3 w-3 text-muted-foreground/60 shrink-0" />
            <span className="font-semibold text-sm text-text-bright truncate">{provider.provider_name}</span>
            <ThermalBadge state={provider.thermal_state} />
            {provider.temp_c !== null && (
              <span className="text-[11px] text-muted-foreground">{fmtTemp(provider.temp_c)}</span>
            )}
            {provider.loaded_models.length > 0 && (
              <span className="text-[11px] text-muted-foreground/60 ml-0.5">
                ({provider.loaded_models.length})
              </span>
            )}
            <div className="ml-auto flex items-center gap-2 shrink-0">
              <span className="text-[11px] text-muted-foreground tabular-nums hidden sm:block">
                {fmtMbShort(provider.used_vram_mb)} / {fmtMbShort(provider.total_vram_mb)}
              </span>
              <div className="w-20">
                <VramBar used={provider.used_vram_mb} total={provider.total_vram_mb} />
              </div>
            </div>
          </div>
        </TableCell>
      </TableRow>
      {!isCollapsed && (
        provider.loaded_models.length === 0 ? (
          <TableRow>
            <TableCell colSpan={4} className="px-10 py-2 text-[11px] text-muted-foreground italic border-b border-border/30">
              {t('providers.capacity.noData')}
            </TableCell>
          </TableRow>
        ) : provider.loaded_models.map((m) => (
          <React.Fragment key={`${provider.provider_id}:${m.model_name}`}>
            <TableRow className="hover:bg-muted/15 transition-colors border-b border-border/30">
              <TableCell className="px-10 py-2 font-mono font-medium text-text-bright">{m.model_name}</TableCell>
              <TableCell className="px-3 py-2 text-right font-mono text-muted-foreground tabular-nums">{fmtMbShort(m.weight_mb)}</TableCell>
              <TableCell className="px-3 py-2 text-right font-mono text-muted-foreground tabular-nums">{fmtMbShort(m.kv_per_request_mb)}</TableCell>
              <TableCell className="px-3 py-2 text-center tabular-nums">
                <span className={m.active_requests > 0 ? 'font-medium text-status-success-fg' : 'text-muted-foreground'}>
                  {m.active_requests}
                </span>
                {m.max_concurrent > 0 && (
                  <span className="text-muted-foreground/50">/{m.max_concurrent}</span>
                )}
              </TableCell>
            </TableRow>
            {m.llm_concern && (
              <TableRow className="bg-status-warning/5 border-b border-border/30">
                <TableCell colSpan={4} className="px-10 py-1.5 text-[11px]">
                  <span className="font-semibold text-status-warning-fg uppercase tracking-wide mr-1.5">
                    {t('providers.capacity.concern')}
                  </span>
                  <span className="text-muted-foreground">{m.llm_concern}</span>
                  {m.llm_reason && (
                    <span className="text-muted-foreground/60 ml-1">— {m.llm_reason}</span>
                  )}
                </TableCell>
              </TableRow>
            )}
          </React.Fragment>
        ))
      )}
    </React.Fragment>
  )
})

export function OllamaCapacitySection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const PROVIDERS_PAGE_SIZE = 20
  const [viewMode, setViewMode] = useState<'server' | 'cluster'>('server')
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [page, setPage] = useState(0)
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300)
    return () => clearTimeout(timer)
  }, [search])
  useEffect(() => { setPage(0) }, [debouncedSearch])

  const { data: capacityData, isLoading: capacityLoading } = useQuery(
    capacityQuery({ search: debouncedSearch || undefined, page: page + 1, limit: PROVIDERS_PAGE_SIZE }),
  )
  const { data: clusterData } = useQuery(capacityClusterQuery)
  const { data: settings } = useQuery(syncSettingsQuery)

  const [analyzerModel, setAnalyzerModel] = useState('')
  const [syncEnabled, setSyncEnabled] = useState(true)
  const [intervalSecs, setIntervalSecs] = useState('')
  const [probePermits, setProbePermits] = useState('1')
  const [probeRate, setProbeRate] = useState('3')

  const prevSettingsRef = useRef<typeof settings>(null)
  useEffect(() => {
    if (settings && prevSettingsRef.current !== settings) {
      prevSettingsRef.current = settings
      setAnalyzerModel(settings.analyzer_model)
      setSyncEnabled(settings.sync_enabled)
      setIntervalSecs(String(settings.sync_interval_secs))
      setProbePermits(String(settings.probe_permits))
      setProbeRate(String(settings.probe_rate))
    }
  }, [settings])

  const saveMutation = useApiMutation(
    (body: PatchSyncSettings) => api.patchSyncSettings(body),
    { invalidateKey: ['sync-settings'] },
  )

  const syncMutation = useMutation({
    mutationFn: () => api.syncAllProviders(),
    onSettled: () => {
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ['capacity'] })
        queryClient.invalidateQueries({ queryKey: ['capacity-cluster'] })
        queryClient.invalidateQueries({ queryKey: ['sync-settings'] })
        queryClient.invalidateQueries({ queryKey: ['providers'] })
        queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
      }, SYNC_INVALIDATE_DELAY_MS)
    },
  })

  const handleSave = () => saveMutation.mutate({
    analyzer_model: analyzerModel || undefined,
    sync_enabled: syncEnabled,
    sync_interval_secs: intervalSecs ? Number(intervalSecs) : undefined,
    probe_permits: probePermits !== '' ? Number(probePermits) : undefined,
    probe_rate: probeRate !== '' ? Number(probeRate) : undefined,
  })

  const toggleCollapsed = useCallback((id: string) => {
    setCollapsed(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }, [])

  const providers = capacityData?.providers ?? []
  const serverTotal = capacityData?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(serverTotal / PROVIDERS_PAGE_SIZE))

  const totalActive = useMemo(
    () => providers.reduce((s, p) => s + p.loaded_models.reduce((a, m) => a + m.active_requests, 0), 0),
    [providers],
  )
  const issueCount = useMemo(
    () => providers.filter(p => p.thermal_state !== 'normal').length,
    [providers],
  )

  const availableModels = useMemo(() =>
    Object.fromEntries(Object.entries(settings?.available_models ?? {}).filter(([p]) => p !== 'gemini' || geminiEnabled)),
    [settings, geminiEnabled],
  )

  function fmtRelativeTime(iso: string | null) {
    if (!iso) return t('providers.capacity.never')
    const mins = Math.floor((Date.now() - new Date(iso).getTime()) / 60_000)
    if (mins < 1) return t('providers.capacity.lessThanMinAgo')
    if (mins < 60) return t('providers.capacity.minsAgo', { n: mins })
    return t('providers.capacity.hoursAgo', { n: Math.floor(mins / 60) })
  }

  return (
    <div className="space-y-4">

      {/* ── 1. 분석기 설정 (상단) ──────────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-3">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <p className="text-sm font-medium">{t('providers.capacity.settings')}</p>
            <div className="flex items-center gap-2">
              {settings?.last_run_at && (
                <span className="text-xs text-muted-foreground">
                  {t('providers.capacity.lastRun')}: {fmtRelativeTime(settings.last_run_at)}
                  {settings.last_run_status && (
                    <span className={`ml-1 font-medium ${settings.last_run_status === 'ok' ? 'text-status-success-fg' : 'text-status-error-fg'}`}>
                      · {settings.last_run_status === 'ok' ? t('providers.capacity.statusOk') : t('providers.capacity.statusError')}
                    </span>
                  )}
                </span>
              )}
              <Button size="sm" variant="outline" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="gap-1.5 shrink-0">
                <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
                {syncMutation.isPending ? t('providers.capacity.syncing') : t('providers.capacity.syncNow')}
              </Button>
            </div>
          </div>

          <div className="flex items-end gap-3 flex-wrap">
            <div className="space-y-1 min-w-44">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.analyzerModel')}</Label>
              <Select value={analyzerModel} onValueChange={setAnalyzerModel}>
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={analyzerModel || '—'} />
                </SelectTrigger>
                <SelectContent>
                  {Object.entries(availableModels).map(([prov, models]) => (
                    <SelectGroup key={prov}>
                      <SelectLabel className="text-[10px] uppercase tracking-wider text-muted-foreground/70">{prov}</SelectLabel>
                      {models.map((m) => (
                        <SelectItem key={`${prov}:${m}`} value={m}>{m}</SelectItem>
                      ))}
                    </SelectGroup>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.interval')}</Label>
              <Input type="number" min={60} className="h-8 text-sm w-24" value={intervalSecs}
                onChange={(e) => setIntervalSecs(e.target.value)} disabled={!syncEnabled} />
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.probePermits')}</Label>
              <Input type="number" className="h-8 text-sm w-20" value={probePermits}
                onChange={(e) => setProbePermits(e.target.value)} />
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.probeRate')}</Label>
              <Input type="number" min={0} className="h-8 text-sm w-20" value={probeRate}
                onChange={(e) => setProbeRate(e.target.value)} />
            </div>
            <div className="flex items-center gap-2 pb-0.5">
              <Switch id="cap-auto" checked={syncEnabled} onCheckedChange={setSyncEnabled} />
              <Label htmlFor="cap-auto" className="text-sm cursor-pointer">{t('providers.capacity.autoAnalysis')}</Label>
            </div>
            <Button size="sm" onClick={handleSave} disabled={saveMutation.isPending} className="pb-0.5">
              {saveMutation.isPending ? t('providers.capacity.saving') : t('common.save')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* ── 2. 툴바: 검색 + 요약 + 뷰 토글 ────────────────────────────────────── */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="relative flex-1 min-w-40 max-w-64">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
          <Input
            className="h-8 text-sm pl-8"
            placeholder={t('providers.capacity.searchProvider')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {!capacityLoading && (
          <div className="flex items-center gap-3 text-xs text-muted-foreground">
            <span className="flex items-center gap-1">
              <Server className="h-3 w-3" />
              <span className="font-medium text-foreground">{serverTotal}</span>
            </span>
            {totalActive > 0 && (
              <span className="flex items-center gap-1 text-status-success-fg">
                <Activity className="h-3 w-3" />
                <span className="font-medium">{totalActive}</span>
              </span>
            )}
            {issueCount > 0 && (
              <span className="flex items-center gap-1 text-status-error-fg">
                <AlertTriangle className="h-3 w-3" />
                <span className="font-medium">{issueCount}</span>
              </span>
            )}
          </div>
        )}

        <div className="ml-auto flex items-center rounded-md border border-border overflow-hidden text-xs">
          <button
            className={`px-2.5 py-1.5 flex items-center gap-1 transition-colors ${viewMode === 'server' ? 'bg-muted text-foreground font-medium' : 'text-muted-foreground hover:text-foreground'}`}
            onClick={() => setViewMode('server')}
          >
            <Server className="h-3 w-3" />{t('providers.capacity.viewServer')}
          </button>
          <button
            className={`px-2.5 py-1.5 flex items-center gap-1 transition-colors border-l border-border ${viewMode === 'cluster' ? 'bg-muted text-foreground font-medium' : 'text-muted-foreground hover:text-foreground'}`}
            onClick={() => setViewMode('cluster')}
          >
            <Layers className="h-3 w-3" />{t('providers.capacity.viewCluster')}
          </button>
        </div>
      </div>

      {capacityLoading && (
        <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
      )}

      {!capacityLoading && providers.length === 0 && (
        <Card className="border-dashed">
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            <Activity className="h-8 w-8 mx-auto mb-2 opacity-25" />
            {t('providers.capacity.noData')}
          </CardContent>
        </Card>
      )}

      {/* ── 3. 클러스터 뷰 ─────────────────────────────────────────────────────── */}
      {viewMode === 'cluster' && (
        <Card>
          <CardContent className="p-0">
            <div className="overflow-x-auto">
              <Table className="text-xs">
                <TableHeader>
                  <TableRow className="border-b border-border bg-muted/30">
                    <TableHead className="px-4 py-2.5 text-left font-medium text-muted-foreground">{t('providers.capacity.colModel')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-right font-medium text-muted-foreground">{t('providers.capacity.colWeight')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-right font-medium text-muted-foreground">{t('providers.capacity.colKvPerReq')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-right font-medium text-muted-foreground">{t('providers.capacity.colProviders')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-center font-medium text-muted-foreground">{t('providers.capacity.colActiveLimit')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody className="divide-y divide-border">
                  {(clusterData ?? []).length === 0 ? (
                    <TableRow><TableCell colSpan={5} className="px-4 py-8 text-center text-muted-foreground italic">{t('providers.capacity.noData')}</TableCell></TableRow>
                  ) : (clusterData ?? []).map((m) => (
                    <TableRow key={m.model_name} className="hover:bg-muted/20 transition-colors">
                      <TableCell className="px-4 py-2.5 font-mono font-medium text-text-bright">{m.model_name}</TableCell>
                      <TableCell className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">{fmtMbShort(m.weight_mb)}</TableCell>
                      <TableCell className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">{fmtMbShort(m.kv_per_request_mb)}</TableCell>
                      <TableCell className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">{m.provider_count}</TableCell>
                      <TableCell className="px-3 py-2.5 text-center tabular-nums text-muted-foreground">
                        {m.total_active}{m.total_limit > 0 ? `/${m.total_limit}` : ''}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </CardContent>
        </Card>
      )}

      {/* ── 4. 서버 뷰 — flat table, 프로바이더 행 클릭으로 접기/펼치기 ───────── */}
      {viewMode === 'server' && providers.length > 0 && (
        <Card>
          <CardContent className="p-0">
            <div className="overflow-x-auto">
              <Table className="text-xs">
                <TableHeader>
                  <TableRow className="border-b border-border bg-muted/30">
                    <TableHead className="px-4 py-2.5 text-left font-medium text-muted-foreground">{t('providers.capacity.colModel')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-right font-medium text-muted-foreground whitespace-nowrap">{t('providers.capacity.colWeight')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-right font-medium text-muted-foreground whitespace-nowrap">{t('providers.capacity.colKvPerReq')}</TableHead>
                    <TableHead className="px-3 py-2.5 text-center font-medium text-muted-foreground whitespace-nowrap">{t('providers.capacity.colActiveLimit')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {providers.map((provider) => (
                    <ProviderRow
                      key={provider.provider_id}
                      provider={provider}
                      isCollapsed={collapsed.has(provider.provider_id)}
                      onToggle={toggleCollapsed}
                    />
                  ))}
                </TableBody>
              </Table>
            </div>
          </CardContent>
        </Card>
      )}

      {/* ── 5. 페이지네이션 ────────────────────────────────────────────────────── */}
      {viewMode === 'server' && totalPages > 1 && (
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {t('providers.capacity.showingProviders', {
              from: page * PROVIDERS_PAGE_SIZE + 1,
              to: Math.min((page + 1) * PROVIDERS_PAGE_SIZE, serverTotal),
              total: serverTotal,
            })}
          </span>
          <div className="flex items-center gap-1">
            <Button size="sm" variant="outline" className="h-7 w-7 p-0" disabled={page === 0}
              onClick={() => setPage(p => p - 1)}>
              <ChevronRight className="h-3.5 w-3.5 rotate-180" />
            </Button>
            <span className="px-2 tabular-nums">{page + 1} / {totalPages}</span>
            <Button size="sm" variant="outline" className="h-7 w-7 p-0" disabled={page + 1 >= totalPages}
              onClick={() => setPage(p => p + 1)}>
              <ChevronRight className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
