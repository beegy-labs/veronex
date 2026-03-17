'use client'

import { useState, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { OllamaSyncJob } from '@/lib/types'
import { ollamaSyncStatusQuery, ollamaModelsQuery } from '@/lib/queries'
import { RotateCcw, Search, Cpu, Server } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { OllamaModelProvidersModal } from './modals'

export { OllamaCapacitySection, ThermalBadge, VramBar } from './ollama-capacity-section'

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
  const filteredModels = useMemo(() =>
    allModels.filter((m) =>
      m.model_name.toLowerCase().includes(search.toLowerCase())
    ),
    [allModels, search],
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

