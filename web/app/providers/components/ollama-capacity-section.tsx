'use client'

import React, { useState, useRef, useMemo, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { PatchSyncSettings } from '@/lib/types'
import { capacityQuery, syncSettingsQuery } from '@/lib/queries'
import { Activity, AlertTriangle, Layers, RefreshCw, Search, Server } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { fmtMbShort, fmtTemp } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { RESOURCE_CRITICAL, RESOURCE_WARNING, SYNC_INVALIDATE_DELAY_MS } from '@/lib/constants'
import { useLabSettings } from '@/components/lab-settings-provider'
import { ProgressBar } from '@/components/progress-bar'

export function ThermalBadge({ state }: { state: 'normal' | 'soft' | 'hard' }) {
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
}

export function VramBar({ used, total }: { used: number; total: number }) {
  const { t } = useTranslation()
  if (total === 0) return <span className="text-xs text-muted-foreground italic">{t('common.na')}</span>
  const pct = Math.min(100, calcPercentage(used, total))
  const color = pct > RESOURCE_CRITICAL ? 'bg-status-error' : pct > RESOURCE_WARNING ? 'bg-status-warning' : 'bg-status-success'
  return (
    <div className="flex items-center gap-2 min-w-32">
      <ProgressBar pct={pct} height="h-2" colorClass={color} trackClass="bg-muted/60" className="flex-1" />
      <span className="text-xs text-muted-foreground tabular-nums shrink-0">{pct}%</span>
    </div>
  )
}

export function OllamaCapacitySection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const PROVIDERS_PAGE_SIZE = 20
  const [viewMode, setViewMode] = useState<'server' | 'cluster'>('server')
  const [providerFilter, setProviderFilter] = useState<string>('')
  const [providerPage, setProviderPage] = useState(0)
  const [debouncedSearch, setDebouncedSearch] = useState('')

  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(providerFilter), 300)
    return () => clearTimeout(t)
  }, [providerFilter])

  const { data: capacityData, isLoading: capacityLoading } = useQuery(
    capacityQuery({ search: debouncedSearch || undefined, page: providerPage + 1, limit: PROVIDERS_PAGE_SIZE }),
  )
  const { data: settings } = useQuery(syncSettingsQuery)
  const [analyzerModel, setAnalyzerModel] = useState<string>('')
  const [syncEnabled, setSyncEnabled] = useState<boolean>(true)
  const [intervalSecs, setIntervalSecs] = useState<string>('')
  const [probePermits, setProbePermits] = useState<string>('1')
  const [probeRate, setProbeRate] = useState<string>('3')

  // Sync local form state when settings load
  const prevSettingsRef = useRef<typeof settings>(null)
  if (settings && prevSettingsRef.current !== settings) {
    prevSettingsRef.current = settings
    setAnalyzerModel(settings.analyzer_model)
    setSyncEnabled(settings.sync_enabled)
    setIntervalSecs(String(settings.sync_interval_secs))
    setProbePermits(String(settings.probe_permits))
    setProbeRate(String(settings.probe_rate))
  }

  const saveMutation = useMutation({
    mutationFn: (body: PatchSyncSettings) => api.patchSyncSettings(body),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['sync-settings'] })
    },
  })

  const syncMutation = useMutation({
    mutationFn: () => api.syncAllProviders(),
    onSettled: () => {
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ['capacity'] })
        queryClient.invalidateQueries({ queryKey: ['sync-settings'] })
        queryClient.invalidateQueries({ queryKey: ['providers'] })
        queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
      }, SYNC_INVALIDATE_DELAY_MS)
    },
  })

  const handleSave = () => {
    const body: PatchSyncSettings = {
      analyzer_model: analyzerModel || undefined,
      sync_enabled: syncEnabled,
      sync_interval_secs: intervalSecs ? Number(intervalSecs) : undefined,
      probe_permits: probePermits !== '' ? Number(probePermits) : undefined,
      probe_rate: probeRate !== '' ? Number(probeRate) : undefined,
    }
    saveMutation.mutate(body)
  }

  useEffect(() => { setProviderPage(0) }, [debouncedSearch])

  const providers = capacityData?.providers ?? []
  const lastRunAt = settings?.last_run_at
  const lastRunStatus = settings?.last_run_status
  const allModels = settings?.available_models ?? {}

  // Providers are already filtered and paginated by the server
  const pagedProviders = providers
  const serverTotal = capacityData?.total ?? 0

  const totalActive = useMemo(() =>
    providers.reduce((sum, p) => sum + p.loaded_models.reduce((s, m) => s + m.active_requests, 0), 0),
    [providers],
  )

  const issueCount = useMemo(() =>
    providers.filter(p => p.thermal_state !== 'normal').length,
    [providers],
  )

  // Cluster view: aggregate loaded models across all providers by model name
  const clusterModels = useMemo(() => {
    const map = new Map<string, { weight_mb: number; kv_per_request_mb: number; active: number; limit: number; providers: number }>()
    for (const p of providers) {
      for (const m of p.loaded_models) {
        const entry = map.get(m.model_name) ?? { weight_mb: m.weight_mb, kv_per_request_mb: m.kv_per_request_mb, active: 0, limit: 0, providers: 0 }
        entry.active += m.active_requests
        entry.limit += m.max_concurrent
        entry.providers++
        map.set(m.model_name, entry)
      }
    }
    return Array.from(map.entries()).sort((a, b) => a[0].localeCompare(b[0]))
  }, [providers])

  // Filter: hide gemini when lab feature is off, apply provider filter
  const availableModels = useMemo(() =>
    Object.fromEntries(
      Object.entries(allModels)
        .filter(([p]) => p !== 'gemini' || geminiEnabled)
        .filter(([p]) => !providerFilter || p.toLowerCase().includes(providerFilter.toLowerCase()))
    ),
    [allModels, geminiEnabled, providerFilter],
  )
  const providerOptions = useMemo(() =>
    Object.keys(allModels).filter(p => p !== 'gemini' || geminiEnabled),
    [allModels, geminiEnabled],
  )

  function fmtRelativeTime(iso: string | null) {
    if (!iso) return t('providers.capacity.never')
    const diff = Date.now() - new Date(iso).getTime()
    const mins = Math.floor(diff / 60_000)
    if (mins < 1) return t('providers.capacity.lessThanMinAgo')
    if (mins < 60) return t('providers.capacity.minsAgo', { n: mins })
    return t('providers.capacity.hoursAgo', { n: Math.floor(mins / 60) })
  }

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <Activity className="h-4 w-4 text-accent-gpu" />
        {t('providers.capacity.title')}
      </h2>

      {/* Settings card */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <p className="text-sm font-medium text-text-bright">{t('providers.capacity.settings')}</p>
            <div className="flex items-center gap-2">
              {lastRunAt && (
                <span className="text-xs text-muted-foreground">
                  {t('providers.capacity.lastRun')}: {fmtRelativeTime(lastRunAt)}
                  {lastRunStatus && (
                    <span className={`ml-1.5 font-medium ${lastRunStatus === 'ok' ? 'text-status-success-fg' : 'text-status-error-fg'}`}>
                      · {lastRunStatus === 'ok' ? t('providers.capacity.statusOk') : t('providers.capacity.statusError')}
                    </span>
                  )}
                </span>
              )}
              <Button
                size="sm"
                variant="outline"
                onClick={() => syncMutation.mutate()}
                disabled={syncMutation.isPending}
                className="gap-1.5 shrink-0"
              >
                <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
                {syncMutation.isPending ? t('providers.capacity.syncing') : t('providers.capacity.syncNow')}
              </Button>
              {syncMutation.isSuccess && !syncMutation.isPending && (
                <span className="text-xs text-status-success-fg">✓ {t('providers.capacity.triggered')}</span>
              )}
            </div>
          </div>

          <div className="flex items-end gap-3 flex-wrap">
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('usage.providerCol')}</Label>
              <div className="relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
                <Input
                  className="h-8 text-sm w-40 pl-7"
                  placeholder={t('providers.capacity.searchProvider')}
                  value={providerFilter}
                  onChange={(e) => setProviderFilter(e.target.value)}
                />
              </div>
            </div>
            <div className="space-y-1 min-w-44">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.analyzerModel')}</Label>
              <Select value={analyzerModel} onValueChange={setAnalyzerModel}>
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={analyzerModel || '—'} />
                </SelectTrigger>
                <SelectContent>
                  {Object.entries(availableModels).map(([provider, models]) => (
                    <SelectGroup key={provider}>
                      <SelectLabel className="text-[10px] uppercase tracking-wider text-muted-foreground/70">
                        {provider}
                      </SelectLabel>
                      {models.map((m) => (
                        <SelectItem key={`${provider}:${m}`} value={m}>{m}</SelectItem>
                      ))}
                    </SelectGroup>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.interval')}</Label>
              <Input
                type="number"
                min={60}
                className="h-8 text-sm w-24"
                value={intervalSecs}
                onChange={(e) => setIntervalSecs(e.target.value)}
                disabled={!syncEnabled}
              />
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.probePermits')}</Label>
              <Input
                type="number"
                className="h-8 text-sm w-20"
                value={probePermits}
                onChange={(e) => setProbePermits(e.target.value)}
              />
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.probeRate')}</Label>
              <Input
                type="number"
                min={0}
                className="h-8 text-sm w-20"
                value={probeRate}
                onChange={(e) => setProbeRate(e.target.value)}
              />
            </div>

            <div className="flex items-center gap-2 pb-0.5">
              <Switch
                id="cap-auto"
                checked={syncEnabled}
                onCheckedChange={setSyncEnabled}
              />
              <Label htmlFor="cap-auto" className="text-sm cursor-pointer">
                {t('providers.capacity.autoAnalysis')}
              </Label>
            </div>

            <Button
              size="sm"
              onClick={handleSave}
              disabled={saveMutation.isPending}
              className="pb-0.5"
            >
              {saveMutation.isPending ? t('providers.capacity.saving') : t('common.save')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* VRAM pool view */}
      {capacityLoading && (
        <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
      )}

      {!capacityLoading && providers.length === 0 && (
        <Card className="border-dashed">
          <CardContent className="p-6 text-center text-sm text-muted-foreground">
            <Activity className="h-8 w-8 mx-auto mb-2 opacity-25" />
            {t('providers.capacity.noData')}
          </CardContent>
        </Card>
      )}

      {providers.length > 0 && (
        <div className="flex items-center justify-between gap-4 px-1">
          <span className="text-xs text-muted-foreground">
            {t('providers.capacity.clusterSummary', { total: serverTotal, active: totalActive, issues: issueCount })}
          </span>
          <div className="flex items-center rounded-md border border-border overflow-hidden text-xs">
            <button
              className={`px-2.5 py-1 flex items-center gap-1 transition-colors ${viewMode === 'server' ? 'bg-muted text-text-bright font-medium' : 'text-muted-foreground hover:text-foreground'}`}
              onClick={() => setViewMode('server')}
            >
              <Server className="h-3 w-3" />서버
            </button>
            <button
              className={`px-2.5 py-1 flex items-center gap-1 transition-colors border-l border-border ${viewMode === 'cluster' ? 'bg-muted text-text-bright font-medium' : 'text-muted-foreground hover:text-foreground'}`}
              onClick={() => setViewMode('cluster')}
            >
              <Layers className="h-3 w-3" />클러스터
            </button>
          </div>
        </div>
      )}

      {/* Cluster aggregate view */}
      {viewMode === 'cluster' && clusterModels.length > 0 && (
        <Card>
          <CardContent className="p-0">
            <div className="overflow-x-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-border bg-muted/30">
                    <th className="px-4 py-2 text-left font-medium text-muted-foreground">{t('providers.capacity.colModel')}</th>
                    <th className="px-3 py-2 text-right font-medium text-muted-foreground">{t('providers.capacity.colWeight')}</th>
                    <th className="px-3 py-2 text-right font-medium text-muted-foreground">{t('providers.capacity.colKvPerReq')}</th>
                    <th className="px-3 py-2 text-right font-medium text-muted-foreground">프로바이더</th>
                    <th className="px-3 py-2 text-center font-medium text-muted-foreground">{t('providers.capacity.colActiveLimit')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {clusterModels.map(([modelName, agg]) => (
                    <tr key={modelName} className="hover:bg-muted/20 transition-colors">
                      <td className="px-4 py-2.5">
                        <span className="font-mono font-medium text-text-bright">{modelName}</span>
                      </td>
                      <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                        {fmtMbShort(agg.weight_mb)}
                      </td>
                      <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                        {fmtMbShort(agg.kv_per_request_mb)}
                      </td>
                      <td className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">
                        {agg.providers}개
                      </td>
                      <td className="px-3 py-2.5 text-center tabular-nums text-muted-foreground">
                        {agg.active}{agg.limit > 0 ? `/${agg.limit}` : ''}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </CardContent>
        </Card>
      )}

      {viewMode === 'server' && pagedProviders.map((provider) => (
        <Card key={provider.provider_id} >
          <CardContent className="p-0">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
              <Server className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
              <span className="text-sm font-semibold text-text-bright">{provider.provider_name}</span>
              <ThermalBadge state={provider.thermal_state} />
              {provider.temp_c !== null && (
                <span className="text-xs text-muted-foreground ml-1">{fmtTemp(provider.temp_c)}</span>
              )}
              <div className="ml-auto flex items-center gap-2">
                <span className="text-xs text-muted-foreground tabular-nums">
                  {fmtMbShort(provider.used_vram_mb)} / {fmtMbShort(provider.total_vram_mb)}
                </span>
                <VramBar used={provider.used_vram_mb} total={provider.total_vram_mb} />
              </div>
            </div>

            {provider.loaded_models.length === 0 ? (
              <p className="px-4 py-4 text-xs text-muted-foreground italic">{t('providers.capacity.noData')}</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-border bg-muted/30">
                      <th className="px-4 py-2 text-left font-medium text-muted-foreground">{t('providers.capacity.colModel')}</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">{t('providers.capacity.colWeight')}</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">{t('providers.capacity.colKvPerReq')}</th>
                      <th className="px-3 py-2 text-center font-medium text-muted-foreground">{t('providers.capacity.colActiveLimit')}</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {provider.loaded_models.map((m) => (
                      <React.Fragment key={m.model_name}>
                        <tr className="hover:bg-muted/20 transition-colors">
                          <td className="px-4 py-2.5">
                            <span className="font-mono font-medium text-text-bright">{m.model_name}</span>
                          </td>
                          <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                            {fmtMbShort(m.weight_mb)}
                          </td>
                          <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                            {fmtMbShort(m.kv_per_request_mb)}
                          </td>
                          <td className="px-3 py-2.5 text-center tabular-nums text-muted-foreground">
                            {m.active_requests}{m.max_concurrent > 0 ? `/${m.max_concurrent}` : ''}
                          </td>
                        </tr>
                        {m.llm_concern && (
                          <tr className="bg-status-warning/5">
                            <td colSpan={4} className="px-4 py-1.5">
                              <span className="text-[10px] font-semibold text-status-warning-fg uppercase tracking-wide mr-2">
                                {t('providers.capacity.concern')}
                              </span>
                              <span className="text-xs text-muted-foreground">{m.llm_concern}</span>
                              {m.llm_reason && (
                                <span className="text-xs text-muted-foreground/70 ml-1">— {m.llm_reason}</span>
                              )}
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </CardContent>
        </Card>
      ))}

      {viewMode === 'server' && serverTotal > PROVIDERS_PAGE_SIZE && (
        <div className="flex items-center justify-between text-xs text-muted-foreground pt-1">
          <span>{t('providers.capacity.showingProviders', { from: providerPage * PROVIDERS_PAGE_SIZE + 1, to: Math.min((providerPage + 1) * PROVIDERS_PAGE_SIZE, serverTotal), total: serverTotal })}</span>
          <div className="flex gap-1">
            <Button size="sm" variant="ghost" disabled={providerPage === 0} onClick={() => setProviderPage(p => p - 1)} aria-label="Previous page">←</Button>
            <Button size="sm" variant="ghost" disabled={(providerPage + 1) * PROVIDERS_PAGE_SIZE >= serverTotal} onClick={() => setProviderPage(p => p + 1)} aria-label="Next page">→</Button>
          </div>
        </div>
      )}
    </div>
  )
}
