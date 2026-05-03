'use client'

import { useState, useOptimistic, startTransition } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Provider, ProviderSelectedModel, OllamaProviderForModel } from '@/lib/types'
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

const PROVIDERS_LIMIT = 10

// Optimistic toggle for a single provider-model pair
function OllamaProviderModelToggle({
  modelName,
  provider,
}: {
  modelName: string
  provider: OllamaProviderForModel
}) {
  const queryClient = useQueryClient()
  const [optimistic, setOptimistic] = useOptimistic(provider.is_enabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (enabled: boolean) => api.setModelEnabled(provider.provider_id, modelName, enabled),
    onError: () => setOptimistic(provider.is_enabled),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['selected-models'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-model-providers', modelName] })
    },
  })
  return (
    <Switch
      checked={optimistic}
      onCheckedChange={(checked) => startTransition(() => { setOptimistic(checked); mutation.mutate(checked) })}
      disabled={mutation.isPending}
      aria-label={`${provider.name} ${modelName} toggle`}
    />
  )
}

export function OllamaModelProvidersModal({ modelName, onClose }: { modelName: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [page, setPage] = useState(1)
  const canManage = hasPermission('model_manage')

  const { data, isLoading } = useQuery(
    ollamaModelProvidersQuery(modelName, { search: debouncedSearch, page, limit: PROVIDERS_LIMIT }),
  )

  const providers = data?.providers ?? []
  const total = data?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / PROVIDERS_LIMIT))
  const pageStart = (page - 1) * PROVIDERS_LIMIT

  function handleSearch(v: string) {
    setSearch(v)
    setPage(1)
    clearTimeout((handleSearch as unknown as { _t?: ReturnType<typeof setTimeout> })._t)
    ;(handleSearch as unknown as { _t?: ReturnType<typeof setTimeout> })._t = setTimeout(
      () => setDebouncedSearch(v),
      300,
    )
  }

  function statusDot(s: string) { return PROVIDER_STATUS_DOT[s] ?? PROVIDER_STATUS_DOT.offline }
  function statusBadgeCls(s: string) { return PROVIDER_STATUS_BADGE[s] ?? PROVIDER_STATUS_BADGE.offline }
  function statusLabel(s: string) {
    const key = PROVIDER_STATUS_I18N[s] ?? PROVIDER_STATUS_I18N.offline
    return t(key)
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-font-mono vds-text-base vds-flex vds-items-center vds-gap-2">
            <Cpu className="vds-h-4 vds-w-4 vds-text-accent-gpu vds-flex-shrink-0" />
            {modelName}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-relative">
          <Search className="vds-absolute vds-left-2.5 vds-top-2.5 vds-h-3.5 vds-w-3.5 vds-text-dim/60 vds-pointer-events-none" />
          <Input
            className="vds-pl-8 vds-h-8 vds-text-sm"
            placeholder={t('providers.ollama.searchServers')}
            value={search}
            onChange={(e) => handleSearch(e.target.value)}
          />
        </div>

        {!isLoading && total > 0 && (
          <p className="vds-text-xs vds-text-dim vds--mt-1">
            {total} {t('providers.ollama.serversWithModel')}
            {debouncedSearch ? ` — "${debouncedSearch}"` : ''}
          </p>
        )}

        {isLoading && (
          <p className="vds-text-sm vds-text-dim vds-py-4 vds-text-center vds-animate-pulse">{t('common.loading')}</p>
        )}

        {!isLoading && total === 0 && !debouncedSearch && (
          <p className="vds-text-sm vds-text-dim vds-py-4 vds-text-center vds-italic">
            {t('providers.ollama.noProvidersSynced')}
          </p>
        )}

        {!isLoading && total === 0 && debouncedSearch && (
          <p className="vds-text-sm vds-text-dim vds-py-3 vds-text-center vds-italic">
            {t('providers.ollama.noServersMatch')} &ldquo;{debouncedSearch}&rdquo;
          </p>
        )}

        {!isLoading && providers.length > 0 && (
          <div className="vds-space-y-2">
            {providers.map((b) => (
              <div key={b.provider_id} className="vds-flex vds-items-center vds-gap-3 vds-rounded-lg vds-border-1 vds-border-subtle vds-px-3 vds-py-2.5">
                <span className={statusDot(b.status)} />
                <div className="vds-min-w-0 vds-flex-1">
                  <p className="vds-text-sm vds-font-500 vds-text-bright vds-truncate">{b.name}</p>
                  <p className="vds-text-xs vds-font-mono vds-text-dim vds-truncate">{extractHost(b.url)}</p>
                </div>
                <Badge variant="outline" className={`vds-whitespace-nowrap ${statusBadgeCls(b.status)}`}>
                  {statusLabel(b.status)}
                </Badge>
                {canManage && (
                  <OllamaProviderModelToggle modelName={modelName} provider={b} />
                )}
              </div>
            ))}
          </div>
        )}

        {totalPages > 1 && (
          <div className="vds-flex vds-items-center vds-justify-between vds-pt-1">
            <span className="vds-text-xs vds-text-dim vds-tabular-nums">
              {pageStart + 1}–{Math.min(pageStart + PROVIDERS_LIMIT, total)} / {total}
            </span>
            <div className="vds-flex vds-items-center vds-gap-1">
              <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                aria-label={t('common.prevPage')}
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={page <= 1}>
                <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
              </Button>
              <span className="vds-text-xs vds-text-dim vds-px-1">
                {page} / {totalPages}
              </span>
              <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                aria-label={t('common.nextPage')}
                onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                disabled={page >= totalPages}>
                <ChevronRight className="vds-h-3.5 vds-w-3.5" />
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

// Optimistic toggle for a single model within a provider
function OllamaProviderModelItemToggle({
  providerId,
  model,
}: {
  providerId: string
  model: ProviderSelectedModel
}) {
  const queryClient = useQueryClient()
  const [optimistic, setOptimistic] = useOptimistic(model.is_enabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (enabled: boolean) => api.setModelEnabled(providerId, model.model_name, enabled),
    onError: () => setOptimistic(model.is_enabled),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['selected-models', providerId] })
    },
  })
  return (
    <Switch
      checked={optimistic}
      onCheckedChange={(checked) => startTransition(() => { setOptimistic(checked); mutation.mutate(checked) })}
      disabled={mutation.isPending}
      aria-label={model.model_name}
    />
  )
}

export function OllamaProviderModelsModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
  const { t } = useTranslation()

  const { data, isLoading } = useQuery(selectedModelsQuery(provider.id))

  const models = data?.models ?? []
  const enabledCount = models.filter((m) => m.is_enabled).length

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <ListFilter className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
            {t('providers.ollama.modelSelection')}
            <span className="vds-text-dim vds-font-400 vds-text-sm">— {provider.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="vds-text-xs vds-text-dim vds--mt-1">
          {t('providers.ollama.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="vds-flex vds-h-20 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="vds-text-sm vds-text-dim vds-py-4 vds-text-center">
            {t('providers.ollama.noProviderModels')}
          </p>
        )}

        {models.length > 0 && (
          <div className="vds-space-y-1 vds-max-h-80 vds-overflow-y-auto vds-pr-1">
            {models.map((m) => (
              <div key={m.model_name}
                className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-3 vds-py-2">
                <span className="vds-font-mono vds-text-sm vds-text-bright">{m.model_name}</span>
                <OllamaProviderModelItemToggle providerId={provider.id} model={m} />
              </div>
            ))}
          </div>
        )}

        {models.length > 0 && (
          <p className="vds-text-xs vds-text-dim vds-text-right">
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
