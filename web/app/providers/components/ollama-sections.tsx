'use client'

import { useState, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { OllamaSyncJob, PatchSyncSettings } from '@/lib/types'
import { ollamaSyncStatusQuery, ollamaModelsQuery, capacityQuery, syncSettingsQuery } from '@/lib/queries'
import { Activity, AlertTriangle, RotateCcw, RefreshCw, Server, Search, Cpu } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { fmtMbShort } from '@/lib/chart-theme'
import { OllamaModelProvidersModal } from './modals'

// ── Shared page size ───────────────────────────────────────────────────────────

export const PAGE_SIZE = 10

// ── Ollama Global Sync Section ─────────────────────────────────────────────────

export function OllamaSyncSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [selectedModel, setSelectedModel] = useState<string | null>(null)

  const { data: syncJob } = useQuery({
    ...ollamaSyncStatusQuery,
    refetchInterval: (query) => {
      const data = query.state.data as OllamaSyncJob | undefined
      return data?.status === 'running' ? 2000 : false
    },
  })

  const { data: ollamaModelsData } = useQuery(ollamaModelsQuery)

  const syncMutation = useMutation({
    mutationFn: () => api.syncOllamaModels(),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['ollama-sync-status'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
    },
  })

  const isRunning = syncJob?.status === 'running' || syncMutation.isPending
  const allModels = ollamaModelsData?.models ?? []
  const filteredModels = allModels.filter((m) =>
    m.model_name.toLowerCase().includes(search.toLowerCase())
  )

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <RotateCcw className="h-4 w-4 text-accent-gpu" />
        {t('providers.ollama.ollamaSyncSection')}
      </h2>

      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={isRunning} className="gap-1.5">
              <RotateCcw className={isRunning ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {isRunning ? t('providers.ollama.ollamaSyncing') : t('providers.ollama.ollamaSyncAll')}
            </Button>
            {syncJob?.status === 'running' && (
              <span className="text-xs text-muted-foreground">
                {syncJob.done_providers}/{syncJob.total_providers}
              </span>
            )}
            {syncJob?.status === 'completed' && !syncMutation.isPending && (
              <span className="text-xs text-status-success-fg">✓ {t('providers.ollama.ollamaSyncDone')}</span>
            )}
          </div>

          {allModels.length === 0 && (
            <p className="text-xs text-muted-foreground italic">{t('providers.ollama.ollamaNoSync')}</p>
          )}

          {allModels.length > 0 && (
            <div className="space-y-3">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground/60 pointer-events-none" />
                <Input
                  className="pl-8 h-8 text-sm"
                  placeholder={t('providers.ollama.ollamaSearchModels')}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </div>
              <div className="flex items-center justify-between">
                <p className="text-xs font-medium text-muted-foreground">
                  {t('providers.ollama.ollamaAvailableModels')}
                </p>
                <span className="text-xs text-muted-foreground">{filteredModels.length}/{allModels.length}</span>
              </div>
              <div className="divide-y divide-border rounded-md border border-border overflow-hidden">
                {filteredModels.length === 0 && (
                  <p className="text-xs text-muted-foreground italic py-3 px-3">
                    {t('providers.ollama.noModelsMatch')} &ldquo;{search}&rdquo;
                  </p>
                )}
                {filteredModels.map((m) => (
                  <button
                    key={m.model_name}
                    className="w-full flex items-center gap-3 px-3 py-2.5 hover:bg-muted/40 transition-colors text-left"
                    onClick={() => setSelectedModel(m.model_name)}
                  >
                    <Cpu className="h-3.5 w-3.5 text-accent-gpu/70 shrink-0" />
                    <span className="font-mono text-sm text-text-bright flex-1 truncate">{m.model_name}</span>
                    <Badge variant="secondary" className="text-[10px] px-1.5 py-0 shrink-0 gap-1">
                      <Server className="h-2.5 w-2.5" />
                      {m.provider_count}
                    </Badge>
                  </button>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {selectedModel && (
        <OllamaModelProvidersModal modelName={selectedModel} onClose={() => setSelectedModel(null)} />
      )}
    </div>
  )
}

// ── Ollama Capacity Section ────────────────────────────────────────────────────

export function ThermalBadge({ state }: { state: 'normal' | 'soft' | 'hard' }) {
  const { t } = useTranslation()
  if (state === 'hard') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-error/15 text-status-error-fg border border-status-error/30">
      <AlertTriangle className="h-2.5 w-2.5" />{t('providers.capacity.thermal.hard')}
    </span>
  )
  if (state === 'soft') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-warn/15 text-status-warn-fg border border-status-warn/30">
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
  if (total === 0) return <span className="text-xs text-muted-foreground italic">unknown</span>
  const pct = Math.min(100, Math.round((used / total) * 100))
  const color = pct > 90 ? 'bg-status-error' : pct > 70 ? 'bg-status-warn' : 'bg-status-success'
  return (
    <div className="flex items-center gap-2 min-w-32">
      <div className="flex-1 h-2 rounded-full bg-muted/60 overflow-hidden">
        <div className={`h-full rounded-full ${color} transition-all`} style={{ width: `${pct}%` }} />
      </div>
      <span className="text-xs text-muted-foreground tabular-nums shrink-0">{pct}%</span>
    </div>
  )
}

export function OllamaCapacitySection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const { data: capacityData, isLoading: capacityLoading } = useQuery(capacityQuery)
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
      }, 3000)
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

  const providers = capacityData?.providers ?? []
  const lastRunAt = settings?.last_run_at
  const lastRunStatus = settings?.last_run_status
  const availableModels = settings?.available_models ?? []

  function fmtRelativeTime(iso: string | null) {
    if (!iso) return t('providers.capacity.never')
    const diff = Date.now() - new Date(iso).getTime()
    const mins = Math.floor(diff / 60_000)
    if (mins < 1) return '< 1 min ago'
    if (mins < 60) return `${mins} min ago`
    return `${Math.floor(mins / 60)}h ago`
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
            <div className="space-y-1 min-w-44">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.analyzerModel')}</Label>
              <Select value={analyzerModel} onValueChange={setAnalyzerModel}>
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={analyzerModel || '—'} />
                </SelectTrigger>
                <SelectContent>
                  {availableModels.map((m) => (
                    <SelectItem key={m} value={m}>{m}</SelectItem>
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
              <Label className="text-xs text-muted-foreground">Probe Permits</Label>
              <Input
                type="number"
                className="h-8 text-sm w-20"
                value={probePermits}
                onChange={(e) => setProbePermits(e.target.value)}
              />
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Probe Rate</Label>
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

      {providers.map((provider) => (
        <Card key={provider.provider_id}>
          <CardContent className="p-0">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
              <Server className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
              <span className="text-sm font-semibold text-text-bright">{provider.provider_name}</span>
              <ThermalBadge state={provider.thermal_state} />
              {provider.temp_c !== null && (
                <span className="text-xs text-muted-foreground ml-1">{provider.temp_c.toFixed(1)}°C</span>
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
                      <th className="px-4 py-2 text-left font-medium text-muted-foreground">Model</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">Weight</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">KV/req</th>
                      <th className="px-3 py-2 text-center font-medium text-muted-foreground">Active / Limit</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {provider.loaded_models.map((m) => (
                      <>
                        <tr key={m.model_name} className="hover:bg-muted/20 transition-colors">
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
                          <tr key={`${m.model_name}-concern`} className="bg-status-warn/5">
                            <td colSpan={4} className="px-4 py-1.5">
                              <span className="text-[10px] font-semibold text-status-warn-fg uppercase tracking-wide mr-2">
                                {t('providers.capacity.concern')}
                              </span>
                              <span className="text-xs text-muted-foreground">{m.llm_concern}</span>
                              {m.llm_reason && (
                                <span className="text-xs text-muted-foreground/70 ml-1">— {m.llm_reason}</span>
                              )}
                            </td>
                          </tr>
                        )}
                      </>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
