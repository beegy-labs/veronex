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
    <span className="vds-inline-flex vds-items-center vds-gap-1 vds-px-2 vds-py-0.5 vds-rounded-full vds-text-[10px] vds-font-600 vds-bg-error/15 vds-text-error vds-border-1 vds-border-error/30">
      <AlertTriangle className="vds-h-2.5 vds-w-2.5" />{t('providers.capacity.thermal.hard')}
    </span>
  )
  if (state === 'soft') return (
    <span className="vds-inline-flex vds-items-center vds-gap-1 vds-px-2 vds-py-0.5 vds-rounded-full vds-text-[10px] vds-font-600 vds-bg-warning/15 vds-text-warning vds-border-1 vds-border-warning/30">
      <AlertTriangle className="vds-h-2.5 vds-w-2.5" />{t('providers.capacity.thermal.soft')}
    </span>
  )
  return (
    <span className="vds-inline-flex vds-items-center vds-gap-1 vds-px-2 vds-py-0.5 vds-rounded-full vds-text-[10px] vds-font-600 vds-bg-success/10 vds-text-success vds-border-1 vds-border-success/30">
      <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success" />{t('providers.capacity.thermal.normal')}
    </span>
  )
})

export const VramBar = memo(function VramBar({ used, total }: { used: number; total: number }) {
  const { t } = useTranslation()
  if (total === 0) return <span className="vds-text-xs vds-text-dim vds-italic">{t('common.na')}</span>
  const pct = Math.min(100, calcPercentage(used, total))
  const color = pct > RESOURCE_CRITICAL ? 'vds-bg-error' : pct > RESOURCE_WARNING ? 'vds-bg-warning' : 'vds-bg-success'
  return (
    <div className="vds-flex vds-items-center vds-gap-2 vds-min-w-24">
      <ProgressBar pct={pct} height="vds-h-1.5" colorClass={color} trackClass="vds-bg-muted/60" className="vds-flex-1" />
      <span className="vds-text-2xs vds-text-dim vds-tabular-nums vds-flex-shrink-0">{pct}%</span>
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
        className="vds-border-t-1 vds-border-subtle vds-bg-muted/40 vds-cursor-pointer vds-hover:bg-hover/60 vds-transition-colors"
        onClick={() => onToggle(provider.provider_id)}
      >
        <TableCell colSpan={4} className="vds-px-3 vds-py-1.5">
          <div className="vds-flex vds-items-center vds-gap-2 vds-min-w-0">
            {isCollapsed
              ? <ChevronRight className="vds-h-3 vds-w-3 vds-text-dim/50 vds-flex-shrink-0" />
              : <ChevronDown className="vds-h-3 vds-w-3 vds-text-dim/50 vds-flex-shrink-0" />
            }
            <Server className="vds-h-3 vds-w-3 vds-text-dim/60 vds-flex-shrink-0" />
            <span className="vds-font-600 vds-text-sm vds-text-bright vds-truncate">{provider.provider_name}</span>
            <ThermalBadge state={provider.thermal_state} />
            {provider.temp_c !== null && (
              <span className="vds-text-2xs vds-text-dim">{fmtTemp(provider.temp_c)}</span>
            )}
            {provider.loaded_models.length > 0 && (
              <span className="vds-text-2xs vds-text-dim/60 vds-ml-0.5">
                ({provider.loaded_models.length})
              </span>
            )}
            <div className="vds-ml-auto vds-flex vds-items-center vds-gap-2 vds-flex-shrink-0">
              <span className="vds-text-2xs vds-text-dim vds-tabular-nums vds-hidden vds-sm:block">
                {fmtMbShort(provider.used_vram_mb)} / {fmtMbShort(provider.total_vram_mb)}
              </span>
              <div className="vds-w-20">
                <VramBar used={provider.used_vram_mb} total={provider.total_vram_mb} />
              </div>
            </div>
          </div>
        </TableCell>
      </TableRow>
      {!isCollapsed && (
        provider.loaded_models.length === 0 ? (
          <TableRow>
            <TableCell colSpan={4} className="vds-px-10 vds-py-2 vds-text-2xs vds-text-dim vds-italic vds-border-b-1 vds-border-subtle/30">
              {t('providers.capacity.noData')}
            </TableCell>
          </TableRow>
        ) : provider.loaded_models.map((m) => (
          <React.Fragment key={`${provider.provider_id}:${m.model_name}`}>
            <TableRow className="vds-hover:bg-hover/15 vds-transition-colors vds-border-b-1 vds-border-subtle/30">
              <TableCell className="vds-px-10 vds-py-2 vds-font-mono vds-font-500 vds-text-bright">{m.model_name}</TableCell>
              <TableCell className="vds-px-3 vds-py-2 vds-text-right vds-font-mono vds-text-dim vds-tabular-nums">{fmtMbShort(m.weight_mb)}</TableCell>
              <TableCell className="vds-px-3 vds-py-2 vds-text-right vds-font-mono vds-text-dim vds-tabular-nums">{fmtMbShort(m.kv_per_request_mb)}</TableCell>
              <TableCell className="vds-px-3 vds-py-2 vds-text-center vds-tabular-nums">
                <span className={m.active_requests > 0 ? 'vds-font-500 vds-text-success' : 'vds-text-dim'}>
                  {m.active_requests}
                </span>
                {m.max_concurrent > 0 && (
                  <span className="vds-text-dim/50">/{m.max_concurrent}</span>
                )}
              </TableCell>
            </TableRow>
            {m.llm_concern && (
              <TableRow className="vds-bg-warning/5 vds-border-b-1 vds-border-subtle/30">
                <TableCell colSpan={4} className="vds-px-10 vds-py-1.5 vds-text-2xs">
                  <span className="vds-font-600 vds-text-warning vds-uppercase vds-tracking-wide vds-mr-1.5">
                    {t('providers.capacity.concern')}
                  </span>
                  <span className="vds-text-dim">{m.llm_concern}</span>
                  {m.llm_reason && (
                    <span className="vds-text-dim/60 vds-ml-1">— {m.llm_reason}</span>
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
    <div className="vds-space-y-4">

      {/* ── 1. 분석기 설정 (상단) ──────────────────────────────────────────────── */}
      <Card>
        <CardContent className="vds-p-4 vds-space-y-3">
          <div className="vds-flex vds-items-center vds-justify-between vds-gap-2 vds-flex-wrap">
            <p className="vds-text-sm vds-font-500">{t('providers.capacity.settings')}</p>
            <div className="vds-flex vds-items-center vds-gap-2">
              {settings?.last_run_at && (
                <span className="vds-text-xs vds-text-dim">
                  {t('providers.capacity.lastRun')}: {fmtRelativeTime(settings.last_run_at)}
                  {settings.last_run_status && (
                    <span className={`vds-ml-1 vds-font-500 ${settings.last_run_status === 'ok' ? 'vds-text-success' : 'vds-text-error'}`}>
                      · {settings.last_run_status === 'ok' ? t('providers.capacity.statusOk') : t('providers.capacity.statusError')}
                    </span>
                  )}
                </span>
              )}
              <Button size="sm" variant="outline" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="vds-gap-1.5 vds-flex-shrink-0">
                <RefreshCw className={syncMutation.isPending ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
                {syncMutation.isPending ? t('providers.capacity.syncing') : t('providers.capacity.syncNow')}
              </Button>
            </div>
          </div>

          <div className="vds-flex vds-items-end vds-gap-3 vds-flex-wrap">
            <div className="vds-space-y-1 vds-min-w-44">
              <Label className="vds-text-xs vds-text-dim">{t('providers.capacity.analyzerModel')}</Label>
              <Select value={analyzerModel} onValueChange={setAnalyzerModel}>
                <SelectTrigger className="vds-h-8 vds-text-sm">
                  <SelectValue placeholder={analyzerModel || '—'} />
                </SelectTrigger>
                <SelectContent>
                  {Object.entries(availableModels).map(([prov, models]) => (
                    <SelectGroup key={prov}>
                      <SelectLabel className="vds-text-[10px] vds-uppercase vds-tracking-wider vds-text-dim/70">{prov}</SelectLabel>
                      {models.map((m) => (
                        <SelectItem key={`${prov}:${m}`} value={m}>{m}</SelectItem>
                      ))}
                    </SelectGroup>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="vds-space-y-1">
              <Label className="vds-text-xs vds-text-dim">{t('providers.capacity.interval')}</Label>
              <Input type="number" min={60} className="vds-h-8 vds-text-sm vds-w-24" value={intervalSecs}
                onChange={(e) => setIntervalSecs(e.target.value)} disabled={!syncEnabled} />
            </div>
            <div className="vds-space-y-1">
              <Label className="vds-text-xs vds-text-dim">{t('providers.capacity.probePermits')}</Label>
              <Input type="number" className="vds-h-8 vds-text-sm vds-w-20" value={probePermits}
                onChange={(e) => setProbePermits(e.target.value)} />
            </div>
            <div className="vds-space-y-1">
              <Label className="vds-text-xs vds-text-dim">{t('providers.capacity.probeRate')}</Label>
              <Input type="number" min={0} className="vds-h-8 vds-text-sm vds-w-20" value={probeRate}
                onChange={(e) => setProbeRate(e.target.value)} />
            </div>
            <div className="vds-flex vds-items-center vds-gap-2 vds-pb-0.5">
              <Switch id="cap-auto" checked={syncEnabled} onCheckedChange={setSyncEnabled} />
              <Label htmlFor="cap-auto" className="vds-text-sm vds-cursor-pointer">{t('providers.capacity.autoAnalysis')}</Label>
            </div>
            <Button size="sm" onClick={handleSave} disabled={saveMutation.isPending} className="vds-pb-0.5">
              {saveMutation.isPending ? t('providers.capacity.saving') : t('common.save')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* ── 2. 툴바: 검색 + 요약 + 뷰 토글 ────────────────────────────────────── */}
      <div className="vds-flex vds-items-center vds-gap-3 vds-flex-wrap">
        <div className="vds-relative vds-flex-1 vds-min-w-40 vds-max-w-64">
          <Search className="vds-absolute vds-left-2.5 vds-top-1/2 -translate-y-1/2 vds-h-3.5 vds-w-3.5 vds-text-dim vds-pointer-events-none" />
          <Input
            className="vds-h-8 vds-text-sm vds-pl-8"
            placeholder={t('providers.capacity.searchProvider')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {!capacityLoading && (
          <div className="vds-flex vds-items-center vds-gap-3 vds-text-xs vds-text-dim">
            <span className="vds-flex vds-items-center vds-gap-1">
              <Server className="vds-h-3 vds-w-3" />
              <span className="vds-font-500 vds-text-primary">{serverTotal}</span>
            </span>
            {totalActive > 0 && (
              <span className="vds-flex vds-items-center vds-gap-1 vds-text-success">
                <Activity className="vds-h-3 vds-w-3" />
                <span className="vds-font-500">{totalActive}</span>
              </span>
            )}
            {issueCount > 0 && (
              <span className="vds-flex vds-items-center vds-gap-1 vds-text-error">
                <AlertTriangle className="vds-h-3 vds-w-3" />
                <span className="vds-font-500">{issueCount}</span>
              </span>
            )}
          </div>
        )}

        <div className="vds-ml-auto vds-flex vds-items-center vds-rounded-md vds-border-1 vds-border-subtle vds-overflow-hidden vds-text-xs">
          <button
            className={`vds-px-2.5 vds-py-1.5 vds-flex vds-items-center vds-gap-1 vds-transition-colors ${viewMode === 'server' ? 'vds-bg-muted vds-text-primary vds-font-medium' : 'vds-text-dim vds-hover:text-primary'}`}
            onClick={() => setViewMode('server')}
          >
            <Server className="vds-h-3 vds-w-3" />{t('providers.capacity.viewServer')}
          </button>
          <button
            className={`vds-px-2.5 vds-py-1.5 vds-flex vds-items-center vds-gap-1 vds-transition-colors vds-border-l-1 vds-border-subtle ${viewMode === 'cluster' ? 'vds-bg-muted vds-text-primary vds-font-medium' : 'vds-text-dim vds-hover:text-primary'}`}
            onClick={() => setViewMode('cluster')}
          >
            <Layers className="vds-h-3 vds-w-3" />{t('providers.capacity.viewCluster')}
          </button>
        </div>
      </div>

      {capacityLoading && (
        <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>
      )}

      {!capacityLoading && providers.length === 0 && (
        <Card className="vds-border-dashed">
          <CardContent className="vds-p-8 vds-text-center vds-text-sm vds-text-dim">
            <Activity className="vds-h-8 vds-w-8 vds-mx-auto vds-mb-2 vds-opacity-25" />
            {t('providers.capacity.noData')}
          </CardContent>
        </Card>
      )}

      {/* ── 3. 클러스터 뷰 ─────────────────────────────────────────────────────── */}
      {viewMode === 'cluster' && (
        <Card>
          <CardContent className="vds-p-0">
            <div className="vds-overflow-x-auto">
              <Table className="vds-text-xs">
                <TableHeader>
                  <TableRow className="vds-border-b-1 vds-border-subtle vds-bg-muted/30">
                    <TableHead className="vds-px-4 vds-py-2.5 vds-text-left vds-font-500 vds-text-dim">{t('providers.capacity.colModel')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim">{t('providers.capacity.colWeight')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim">{t('providers.capacity.colKvPerReq')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim">{t('providers.capacity.colProviders')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-center vds-font-500 vds-text-dim">{t('providers.capacity.colActiveLimit')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody className="vds-divide-y vds-divide-border">
                  {(clusterData ?? []).length === 0 ? (
                    <TableRow><TableCell colSpan={5} className="vds-px-4 vds-py-8 vds-text-center vds-text-dim vds-italic">{t('providers.capacity.noData')}</TableCell></TableRow>
                  ) : (clusterData ?? []).map((m) => (
                    <TableRow key={m.model_name} className="vds-hover:bg-hover/20 vds-transition-colors">
                      <TableCell className="vds-px-4 vds-py-2.5 vds-font-mono vds-font-500 vds-text-bright">{m.model_name}</TableCell>
                      <TableCell className="vds-px-3 vds-py-2.5 vds-text-right vds-font-mono vds-text-dim vds-tabular-nums">{fmtMbShort(m.weight_mb)}</TableCell>
                      <TableCell className="vds-px-3 vds-py-2.5 vds-text-right vds-font-mono vds-text-dim vds-tabular-nums">{fmtMbShort(m.kv_per_request_mb)}</TableCell>
                      <TableCell className="vds-px-3 vds-py-2.5 vds-text-right vds-tabular-nums vds-text-dim">{m.provider_count}</TableCell>
                      <TableCell className="vds-px-3 vds-py-2.5 vds-text-center vds-tabular-nums vds-text-dim">
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
          <CardContent className="vds-p-0">
            <div className="vds-overflow-x-auto">
              <Table className="vds-text-xs">
                <TableHeader>
                  <TableRow className="vds-border-b-1 vds-border-subtle vds-bg-muted/30">
                    <TableHead className="vds-px-4 vds-py-2.5 vds-text-left vds-font-500 vds-text-dim">{t('providers.capacity.colModel')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('providers.capacity.colWeight')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('providers.capacity.colKvPerReq')}</TableHead>
                    <TableHead className="vds-px-3 vds-py-2.5 vds-text-center vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('providers.capacity.colActiveLimit')}</TableHead>
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
        <div className="vds-flex vds-items-center vds-justify-between vds-text-xs vds-text-dim">
          <span>
            {t('providers.capacity.showingProviders', {
              from: page * PROVIDERS_PAGE_SIZE + 1,
              to: Math.min((page + 1) * PROVIDERS_PAGE_SIZE, serverTotal),
              total: serverTotal,
            })}
          </span>
          <div className="vds-flex vds-items-center vds-gap-1">
            <Button size="sm" variant="outline" className="vds-h-7 vds-w-7 vds-p-0" disabled={page === 0}
              onClick={() => setPage(p => p - 1)}>
              <ChevronRight className="vds-h-3.5 vds-w-3.5 vds-rotate-180" />
            </Button>
            <span className="vds-px-2 vds-tabular-nums">{page + 1} / {totalPages}</span>
            <Button size="sm" variant="outline" className="vds-h-7 vds-w-7 vds-p-0" disabled={page + 1 >= totalPages}
              onClick={() => setPage(p => p + 1)}>
              <ChevronRight className="vds-h-3.5 vds-w-3.5" />
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
