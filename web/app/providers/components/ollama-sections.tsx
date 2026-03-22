'use client'

import { useState, useMemo, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { OllamaSyncJob } from '@/lib/types'
import { ollamaSyncStatusQuery, ollamaModelsQuery } from '@/lib/queries'
import { RotateCcw, Search, Cpu, Server, ChevronLeft, ChevronRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { hasPermission } from '@/lib/auth'
import { OllamaModelProvidersModal } from './modals'

export { OllamaCapacitySection, ThermalBadge, VramBar } from './ollama-capacity-section'

// ── Shared page size ───────────────────────────────────────────────────────────

export const PAGE_SIZE = 10
const MODEL_LIMIT = 20

// ── Ollama Global Sync Section ─────────────────────────────────────────────────

export function OllamaSyncSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [page, setPage] = useState(1)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [selectedModel, setSelectedModel] = useState<string | null>(null)

  const { data: syncJob } = useQuery({
    ...ollamaSyncStatusQuery,
    refetchInterval: (query) => {
      const data = query.state.data as OllamaSyncJob | undefined
      return data?.status === 'running' ? 2000 : false
    },
  })

  const { data: ollamaModelsData } = useQuery(ollamaModelsQuery({ search: debouncedSearch, page, limit: MODEL_LIMIT }))

  const { data: globalSettings } = useQuery({
    queryKey: ['global-model-settings'],
    queryFn: () => api.globalModelSettings(),
  })

  const globalDisabledSet = useMemo(
    () => new Set<string>((globalSettings ?? []).filter(s => !s.is_enabled).map(s => s.model_name)),
    [globalSettings]
  )

  const canManageModels = hasPermission('model_manage')

  const toggleGlobalMutation = useMutation({
    mutationFn: ({ model, enabled }: { model: string; enabled: boolean }) =>
      api.setGlobalModelEnabled(model, enabled),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['global-model-settings'] }),
  })

  const syncMutation = useMutation({
    mutationFn: () => api.syncOllamaModels(),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['ollama-sync-status'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
    },
  })

  const isRunning = syncJob?.status === 'running' || syncMutation.isPending
  const models = ollamaModelsData?.models ?? []
  const total = ollamaModelsData?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / MODEL_LIMIT))

  useEffect(() => () => { if (debounceRef.current) clearTimeout(debounceRef.current) }, [])

  function handleSearch(v: string) {
    setSearch(v)
    setPage(1)
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => setDebouncedSearch(v), 300)
  }

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

          {total === 0 && !debouncedSearch && (
            <p className="text-xs text-muted-foreground italic">{t('providers.ollama.ollamaNoSync')}</p>
          )}

          {(total > 0 || debouncedSearch) && (
            <div className="space-y-3">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground/60 pointer-events-none" />
                <Input
                  className="pl-8 h-8 text-sm"
                  placeholder={t('providers.ollama.ollamaSearchModels')}
                  value={search}
                  onChange={(e) => handleSearch(e.target.value)}
                />
              </div>
              <div className="flex items-center justify-between">
                <p className="text-xs font-medium text-muted-foreground">
                  {t('providers.ollama.ollamaAvailableModels')}
                </p>
                <span className="text-xs text-muted-foreground tabular-nums">{total}</span>
              </div>
              <div className="divide-y divide-border rounded-md border border-border overflow-hidden">
                {models.length === 0 && debouncedSearch && (
                  <p className="text-xs text-muted-foreground italic py-3 px-3">
                    {t('providers.ollama.noModelsMatch')} &ldquo;{debouncedSearch}&rdquo;
                  </p>
                )}
                {models.map((m) => {
                  const isDisabled = globalDisabledSet.has(m.model_name)
                  return (
                    <div
                      key={m.model_name}
                      className={`flex items-center gap-3 px-3 py-2.5 hover:bg-muted/40 transition-colors ${isDisabled ? 'opacity-50' : ''}`}
                    >
                      <button
                        className="flex items-center gap-3 flex-1 text-left min-w-0"
                        onClick={() => setSelectedModel(m.model_name)}
                      >
                        <Cpu className="h-3.5 w-3.5 text-accent-gpu/70 shrink-0" />
                        <span className="font-mono text-sm text-text-bright flex-1 truncate">{m.model_name}</span>
                      </button>
                      <Badge variant="secondary" className="text-[10px] px-1.5 py-0 shrink-0 gap-1 whitespace-nowrap">
                        <Server className="h-2.5 w-2.5" />
                        {m.provider_count}
                      </Badge>
                      {isDisabled && (
                        <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-status-error-fg border-status-error/30 whitespace-nowrap">
                          {t('common.disabled')}
                        </Badge>
                      )}
                      {canManageModels && (
                        <Switch
                          checked={!isDisabled}
                          onCheckedChange={(checked) =>
                            toggleGlobalMutation.mutate({ model: m.model_name, enabled: checked })
                          }
                          disabled={toggleGlobalMutation.isPending}
                          aria-label={t('providers.ollama.modelToggle', { model: m.model_name })}
                        />
                      )}
                    </div>
                  )
                })}
              </div>
              {totalPages > 1 && (
                <div className="flex items-center justify-end gap-1 mt-2">
                  <span className="text-xs text-muted-foreground tabular-nums mr-2">
                    {(page - 1) * MODEL_LIMIT + 1}–{Math.min(page * MODEL_LIMIT, total)} / {total}
                  </span>
                  <Button variant="outline" size="icon" className="h-7 w-7" disabled={page <= 1}
                    aria-label={t('common.prevPage')} onClick={() => setPage(p => p - 1)}>
                    <ChevronLeft className="h-3.5 w-3.5" />
                  </Button>
                  <Button variant="outline" size="icon" className="h-7 w-7" disabled={page >= totalPages}
                    aria-label={t('common.nextPage')} onClick={() => setPage(p => p + 1)}>
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                </div>
              )}
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
