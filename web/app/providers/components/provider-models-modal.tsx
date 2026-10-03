'use client'

import { useOptimistic, startTransition } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { ListFilter } from 'lucide-react'
import { api } from '@/lib/api'
import { selectedModelsQuery } from '@/lib/queries'
import { PROVIDER_GEMINI } from '@/lib/constants'
import type { Provider, ProviderSelectedModel } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'

function ProviderModelToggle({ providerId, model }: { providerId: string; model: ProviderSelectedModel }) {
  const queryClient = useQueryClient()
  const [optimistic, setOptimistic] = useOptimistic(model.is_enabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (enabled: boolean) => api.setModelEnabled(providerId, model.model_name, enabled),
    onError: () => setOptimistic(model.is_enabled),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: selectedModelsQuery(providerId).queryKey })
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

// ── Model selection modal ──────────────────────────────────────────────────────

export function ProviderModelsModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
  const { t } = useTranslation()
  const isGemini = provider.provider_type === PROVIDER_GEMINI

  const { data, isLoading } = useQuery(selectedModelsQuery(provider.id))

  const models = data?.models ?? []
  const enabledCount = models.filter((m) => m.is_enabled).length

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <ListFilter className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
            {t(isGemini ? 'providers.gemini.modelSelection' : 'providers.llama_server.modelSelection')}
            <span className="vds-text-dim vds-font-400 vds-text-sm">— {provider.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="vds-text-xs vds-text-dim vds--mt-1">
          {t(isGemini ? 'providers.gemini.modelSelectionDesc' : 'providers.llama_server.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="vds-flex vds-h-20 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="vds-text-sm vds-text-dim vds-py-4 vds-text-center">
            {t(isGemini ? 'providers.gemini.noGlobalModels' : 'providers.llama_server.noProviderModels')}
          </p>
        )}

        {models.length > 0 && (
          <div className="vds-space-y-1 vds-max-h-80 vds-overflow-y-auto vds-pr-1">
            {models.map((m) => (
              <div key={m.model_name}
                className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-3 vds-py-2">
                <span className="vds-font-mono vds-text-sm vds-text-bright">{m.model_name}</span>
                <ProviderModelToggle providerId={provider.id} model={m} />
              </div>
            ))}
          </div>
        )}

        {models.length > 0 && (
          <p className="vds-text-xs vds-text-dim vds-text-right">
            {t(isGemini ? 'providers.gemini.modelsCount' : 'providers.llama_server.enabledCount', { enabled: enabledCount, total: models.length })}
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
