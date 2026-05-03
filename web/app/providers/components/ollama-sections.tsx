'use client'

import { useState, useEffect, useRef, useOptimistic } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { OllamaSyncJob } from '@/lib/types'
import { ollamaSyncStatusQuery, ollamaModelsQuery } from '@/lib/queries'
import { useGlobalDisabledSet } from '@/hooks/use-enabled-ollama-models'
import { withJitter } from '@/lib/constants'
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

// ── Global model toggle with optimistic update ─────────────────────────────────

function GlobalModelToggle({ modelName, isEnabled }: { modelName: string; isEnabled: boolean }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [optimistic, setOptimistic] = useOptimistic(isEnabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (enabled: boolean) => api.setGlobalModelEnabled(modelName, enabled),
    onError: () => setOptimistic(isEnabled),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['global-model-settings'] }),
  })
  return (
    <Switch
      checked={optimistic}
      onCheckedChange={(checked) => { setOptimistic(checked); mutation.mutate(checked) }}
      aria-label={t('providers.ollama.modelToggle', { model: modelName })}
    />
  )
}

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
      return data?.status === 'running' ? withJitter(2000, 200) : false
    },
  })

  const { data: ollamaModelsData } = useQuery(ollamaModelsQuery({ search: debouncedSearch, page, limit: MODEL_LIMIT }))

  // This section is where operators *manage* the global disable set, so it must
  // still show disabled models (with a "disabled" badge). Re-uses the shared
  // derivation so the list stays consistent with every picker.
  const { disabledSet: globalDisabledSet } = useGlobalDisabledSet()

  const canManageModels = hasPermission('model_manage')

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
    <div className="vds-space-y-3">
      <h2 className="vds-text-base vds-font-600 vds-text-bright vds-flex vds-items-center vds-gap-2">
        <RotateCcw className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
        {t('providers.ollama.ollamaSyncSection')}
      </h2>

      <Card>
        <CardContent className="vds-p-4 vds-space-y-4">
          <div className="vds-flex vds-items-center vds-gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={isRunning} className="vds-gap-1.5">
              <RotateCcw className={isRunning ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
              {isRunning ? t('providers.ollama.ollamaSyncing') : t('providers.ollama.ollamaSyncAll')}
            </Button>
            {syncJob?.status === 'running' && (
              <span className="vds-text-xs vds-text-dim">
                {syncJob.done_providers}/{syncJob.total_providers}
              </span>
            )}
            {syncJob?.status === 'completed' && !syncMutation.isPending && (
              <span className="vds-text-xs vds-text-success">✓ {t('providers.ollama.ollamaSyncDone')}</span>
            )}
          </div>

          {total === 0 && !debouncedSearch && (
            <p className="vds-text-xs vds-text-dim vds-italic">{t('providers.ollama.ollamaNoSync')}</p>
          )}

          {(total > 0 || debouncedSearch) && (
            <div className="vds-space-y-3">
              <div className="vds-relative">
                <Search className="vds-absolute vds-left-2.5 vds-top-2.5 vds-h-3.5 vds-w-3.5 vds-text-dim/60 vds-pointer-events-none" />
                <Input
                  className="vds-pl-8 vds-h-8 vds-text-sm"
                  placeholder={t('providers.ollama.ollamaSearchModels')}
                  value={search}
                  onChange={(e) => handleSearch(e.target.value)}
                />
              </div>
              <div className="vds-flex vds-items-center vds-justify-between">
                <p className="vds-text-xs vds-font-500 vds-text-dim">
                  {t('providers.ollama.ollamaAvailableModels')}
                </p>
                <span className="vds-text-xs vds-text-dim vds-tabular-nums">{total}</span>
              </div>
              <div className="vds-divide-y vds-divide-border vds-rounded-md vds-border-1 vds-border-subtle vds-overflow-hidden">
                {models.length === 0 && debouncedSearch && (
                  <p className="vds-text-xs vds-text-dim vds-italic vds-py-3 vds-px-3">
                    {t('providers.ollama.noModelsMatch')} &ldquo;{debouncedSearch}&rdquo;
                  </p>
                )}
                {models.map((m) => {
                  const isDisabled = globalDisabledSet.has(m.model_name)
                  return (
                    <div
                      key={m.model_name}
                      className={`vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2.5 vds-hover:bg-hover/40 vds-transition-colors ${isDisabled ? 'vds-opacity-50' : ''}`}
                    >
                      <button
                        className="vds-flex vds-items-center vds-gap-3 vds-flex-1 vds-text-left vds-min-w-0"
                        onClick={() => setSelectedModel(m.model_name)}
                      >
                        <Cpu className="vds-h-3.5 vds-w-3.5 vds-text-accent-gpu/70 vds-flex-shrink-0" />
                        <span className="vds-font-mono vds-text-sm vds-text-bright vds-flex-1 vds-truncate">{m.model_name}</span>
                      </button>
                      <Badge variant="secondary" className="vds-text-[10px] vds-px-1.5 vds-py-0 vds-flex-shrink-0 vds-gap-1 vds-whitespace-nowrap">
                        <Server className="vds-h-2.5 vds-w-2.5" />
                        {m.provider_count}
                      </Badge>
                      {isDisabled && (
                        <Badge variant="outline" className="vds-text-[10px] vds-px-1.5 vds-py-0 vds-text-error vds-border-error/30 vds-whitespace-nowrap">
                          {t('common.disabled')}
                        </Badge>
                      )}
                      {canManageModels && (
                        <GlobalModelToggle modelName={m.model_name} isEnabled={!isDisabled} />
                      )}
                    </div>
                  )
                })}
              </div>
              {totalPages > 1 && (
                <div className="vds-flex vds-items-center vds-justify-end vds-gap-1 vds-mt-2">
                  <span className="vds-text-xs vds-text-dim vds-tabular-nums vds-mr-2">
                    {(page - 1) * MODEL_LIMIT + 1}–{Math.min(page * MODEL_LIMIT, total)} / {total}
                  </span>
                  <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={page <= 1}
                    aria-label={t('common.prevPage')} onClick={() => setPage(p => p - 1)}>
                    <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
                  </Button>
                  <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={page >= totalPages}
                    aria-label={t('common.nextPage')} onClick={() => setPage(p => p + 1)}>
                    <ChevronRight className="vds-h-3.5 vds-w-3.5" />
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
