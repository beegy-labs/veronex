'use client'

import { useState, useMemo } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Provider, ProviderSelectedModel } from '@/lib/types'
import { selectedModelsQuery, ollamaModelProvidersQuery } from '@/lib/queries'
import { Search, Cpu, ChevronLeft, ChevronRight, ListFilter } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'
import { hasPermission } from '@/lib/auth'
import {
  PROVIDER_STATUS_DOT, PROVIDER_STATUS_BADGE, PROVIDER_STATUS_I18N,
} from '@/lib/constants'
import { extractHost } from './shared'

// ── OllamaModelProvidersModal ───────────────────────────────────────────────────

const PROVIDERS_PAGE_SIZE = 8

export function OllamaModelProvidersModal({ modelName, onClose }: { modelName: string; onClose: () => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)
  const canManage = hasPermission('model_manage')

  const { data, isLoading } = useQuery(ollamaModelProvidersQuery(modelName))

  const toggleModelMutation = useMutation({
    mutationFn: ({ providerId, enabled }: { providerId: string; enabled: boolean }) =>
      api.setModelEnabled(providerId, modelName, enabled),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['selected-models'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-model-providers', modelName] })
    },
  })

  const allProviders = data?.providers ?? []
  const { filtered, totalPages, safePage, pageStart, pageItems } = useMemo(() => {
    const filtered = allProviders.filter((b) =>
      b.name.toLowerCase().includes(search.toLowerCase()) ||
      b.url.toLowerCase().includes(search.toLowerCase())
    )
    const totalPages = Math.max(1, Math.ceil(filtered.length / PROVIDERS_PAGE_SIZE))
    const safePage = Math.min(page, totalPages)
    const pageStart = (safePage - 1) * PROVIDERS_PAGE_SIZE
    const pageItems = filtered.slice(pageStart, pageStart + PROVIDERS_PAGE_SIZE)
    return { filtered, totalPages, safePage, pageStart, pageItems }
  }, [allProviders, search, page])

  const handleSearch = (v: string) => { setSearch(v); setPage(1) }

  function statusDot(s: string) { return PROVIDER_STATUS_DOT[s] ?? PROVIDER_STATUS_DOT.offline }
  function statusBadgeCls(s: string) { return PROVIDER_STATUS_BADGE[s] ?? PROVIDER_STATUS_BADGE.offline }
  function statusLabel(s: string) {
    const key = PROVIDER_STATUS_I18N[s] ?? PROVIDER_STATUS_I18N.offline
    return t(key)
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="font-mono text-base flex items-center gap-2">
            <Cpu className="h-4 w-4 text-accent-gpu shrink-0" />
            {modelName}
          </DialogTitle>
        </DialogHeader>

        <div className="relative">
          <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground/60 pointer-events-none" />
          <Input
            className="pl-8 h-8 text-sm"
            placeholder={t('providers.ollama.searchServers')}
            value={search}
            onChange={(e) => handleSearch(e.target.value)}
          />
        </div>

        {!isLoading && allProviders.length > 0 && (
          <p className="text-xs text-muted-foreground -mt-1">
            {filtered.length} / {allProviders.length} {t('providers.ollama.serversWithModel')}
            {search ? ` — "${search}"` : ''}
          </p>
        )}

        {isLoading && (
          <p className="text-sm text-muted-foreground py-4 text-center animate-pulse">{t('common.loading')}</p>
        )}

        {!isLoading && allProviders.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center italic">
            {t('providers.ollama.noBackendsSynced')}
          </p>
        )}

        {!isLoading && filtered.length === 0 && search && (
          <p className="text-sm text-muted-foreground py-3 text-center italic">
            {t('providers.ollama.noServersMatch')} &ldquo;{search}&rdquo;
          </p>
        )}

        {!isLoading && pageItems.length > 0 && (
          <div className="space-y-2">
            {pageItems.map((b) => (
              <div key={b.provider_id} className="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5">
                <span className={statusDot(b.status)} />
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium text-text-bright truncate">{b.name}</p>
                  <p className="text-xs font-mono text-muted-foreground truncate">{extractHost(b.url)}</p>
                </div>
                <Badge variant="outline" className={`whitespace-nowrap ${statusBadgeCls(b.status)}`}>
                  {statusLabel(b.status)}
                </Badge>
                {canManage && (
                  <Switch
                    checked={b.is_enabled !== false}
                    onCheckedChange={(checked) =>
                      toggleModelMutation.mutate({ providerId: b.provider_id, enabled: checked })
                    }
                    disabled={toggleModelMutation.isPending}
                    aria-label={`${b.name} ${modelName} toggle`}
                  />
                )}
              </div>
            ))}
          </div>
        )}

        {totalPages > 1 && (
          <div className="flex items-center justify-between pt-1">
            <span className="text-xs text-muted-foreground">
              {pageStart + 1}–{Math.min(pageStart + PROVIDERS_PAGE_SIZE, filtered.length)} / {filtered.length}
            </span>
            <div className="flex items-center gap-1">
              <Button variant="outline" size="icon" className="h-7 w-7"
                aria-label={t('common.prevPage')}
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={safePage <= 1}>
                <ChevronLeft className="h-3.5 w-3.5" />
              </Button>
              <span className="text-xs text-muted-foreground px-1">
                {safePage} / {totalPages}
              </span>
              <Button variant="outline" size="icon" className="h-7 w-7"
                aria-label={t('common.nextPage')}
                onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                disabled={safePage >= totalPages}>
                <ChevronRight className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── OllamaProviderModelsModal ───────────────────────────────────────────────────

export function OllamaProviderModelsModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const { data, isLoading } = useQuery(selectedModelsQuery(provider.id))

  const toggleMutation = useMutation({
    mutationFn: ({ modelName, isEnabled }: { modelName: string; isEnabled: boolean }) =>
      api.setModelEnabled(provider.id, modelName, isEnabled),
    onMutate: async ({ modelName, isEnabled }) => {
      await queryClient.cancelQueries({ queryKey: ['selected-models', provider.id] })
      const prev = queryClient.getQueryData<{ models: ProviderSelectedModel[] }>(['selected-models', provider.id])
      queryClient.setQueryData<{ models: ProviderSelectedModel[] }>(['selected-models', provider.id], (old) => {
        if (!old) return old
        return {
          models: old.models.map((m) =>
            m.model_name === modelName ? { ...m, is_enabled: isEnabled } : m,
          ),
        }
      })
      return { prev }
    },
    onError: (_err, _vars, context) => {
      if (context?.prev) {
        queryClient.setQueryData(['selected-models', provider.id], context.prev)
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['selected-models', provider.id] })
    },
  })

  const models = data?.models ?? []
  const enabledCount = models.filter((m) => m.is_enabled).length

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ListFilter className="h-4 w-4 text-accent-gpu" />
            {t('providers.ollama.modelSelection')}
            <span className="text-muted-foreground font-normal text-sm">— {provider.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="text-xs text-muted-foreground -mt-1">
          {t('providers.ollama.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="flex h-20 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center">
            {t('providers.ollama.noBackendModels')}
          </p>
        )}

        {models.length > 0 && (
          <div className="space-y-1 max-h-80 overflow-y-auto pr-1">
            {models.map((m) => (
              <div key={m.model_name}
                className="flex items-center justify-between rounded-lg border border-border px-3 py-2">
                <span className="font-mono text-sm text-text-bright">{m.model_name}</span>
                <Switch
                  checked={m.is_enabled}
                  onCheckedChange={(checked) =>
                    toggleMutation.mutate({ modelName: m.model_name, isEnabled: checked })
                  }
                  disabled={toggleMutation.isPending}
                  aria-label={m.model_name}
                />
              </div>
            ))}
          </div>
        )}

        {models.length > 0 && (
          <p className="text-xs text-muted-foreground text-right">
            {t('providers.ollama.enabledCount', { enabled: enabledCount, total: models.length })}
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
