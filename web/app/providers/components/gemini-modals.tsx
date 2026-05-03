'use client'

import { useState, useOptimistic, startTransition } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { api } from '@/lib/api'
import type { Provider, ProviderSelectedModel, GeminiRateLimitPolicy } from '@/lib/types'
import { selectedModelsQuery, providerKeyQuery } from '@/lib/queries'
import { Key, ShieldCheck, Eye, EyeOff, ListFilter } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'
import { GEMINI_QUERY_KEYS } from '@/lib/queries/providers'

// ── Gemini rate limit policy modal ─────────────────────────────────────────────

export function EditPolicyModal({ policy, onClose }: { policy: GeminiRateLimitPolicy; onClose: () => void }) {
  const { t } = useTranslation()
  const [rpm, setRpm] = useState(String(policy.rpm_limit))
  const [rpd, setRpd] = useState(String(policy.rpd_limit))
  const [availableOnFreeTier, setAvailableOnFreeTier] = useState(policy.available_on_free_tier)
  const mutation = useApiMutation(
    () => api.upsertGeminiPolicy(policy.model_name, {
      rpm_limit: rpm ? parseInt(rpm, 10) : 0,
      rpd_limit: rpd ? parseInt(rpd, 10) : 0,
      available_on_free_tier: availableOnFreeTier,
    }),
    { invalidateKey: ['gemini-policies'], onSuccess: () => onClose() },
  )

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-sm">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <ShieldCheck className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
            {t('providers.gemini.editPolicyTitle')}
          </DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-1 vds-mb-1">
          <p className="vds-text-sm vds-text-dim">{t('providers.gemini.model')}</p>
          <p className="vds-font-mono vds-text-sm vds-font-600 vds-text-bright">
            {policy.model_name === '*' ? `* (${t('providers.gemini.globalDefault')})` : policy.model_name}
          </p>
        </div>

        <div className="vds-space-y-4">
          <div className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-4 vds-py-3">
            <div>
              <p className="vds-text-sm vds-font-500">{t('providers.gemini.availableOnFreeTier')}</p>
              <p className="vds-text-xs vds-text-dim vds-mt-0.5">
                {availableOnFreeTier
                  ? t('providers.gemini.freeTierRouting')
                  : t('providers.gemini.paidOnlyRouting')}
              </p>
            </div>
            <Switch checked={availableOnFreeTier} onCheckedChange={setAvailableOnFreeTier} aria-label={t('providers.gemini.availableOnFreeTier')} />
          </div>

          {availableOnFreeTier && (
            <div className="vds-grid vds-grid-cols-1 vds-sm:grid-cols-2 vds-gap-3">
              <div className="vds-space-y-1.5">
                <Label htmlFor="pol-rpm" className="vds-text-xs">{t('providers.gemini.rpm')} <span className="vds-text-dim vds-font-400">({t('providers.gemini.rpmUnit')})</span></Label>
                <Input id="pol-rpm" type="number" min={0} value={rpm}
                  onChange={(e) => setRpm(e.target.value)} placeholder={t('providers.gemini.rpmPlaceholder')} className="vds-h-8 vds-text-sm" />
              </div>
              <div className="vds-space-y-1.5">
                <Label htmlFor="pol-rpd" className="vds-text-xs">{t('providers.gemini.rpd')} <span className="vds-text-dim vds-font-400">({t('providers.gemini.rpdUnit')})</span></Label>
                <Input id="pol-rpd" type="number" min={0} value={rpd}
                  onChange={(e) => setRpd(e.target.value)} placeholder={t('providers.gemini.rpdPlaceholder')} className="vds-h-8 vds-text-sm" />
              </div>
              <p className="vds-col-span-2 vds-text-2xs vds-text-dim vds--mt-1">
                {t('providers.gemini.freeLimitsHint')}
              </p>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('providers.gemini.failedToSave')}
          </p>
        )}
        <DialogFooter className="vds-gap-3 vds-flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate(undefined)} disabled={mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Key reveal cell ────────────────────────────────────────────────────────────

export function ApiKeyCell({ providerId, masked }: { providerId: string; masked: string | null }) {
  const { t } = useTranslation()
  const [revealed, setRevealed] = useState(false)

  const { data, isFetching, refetch } = useQuery(providerKeyQuery(providerId))

  async function handleReveal() {
    if (revealed) { setRevealed(false); return }
    if (data) { setRevealed(true); return }
    await refetch()
    setRevealed(true)
  }

  const displayKey = revealed && data?.key ? data.key : (masked ?? '—')

  return (
    <div className="vds-flex vds-items-center vds-gap-1.5">
      <span className="vds-font-mono vds-text-xs vds-text-dim vds-select-all">{displayKey}</span>
      {masked && (
        <Button variant="ghost" size="icon"
          className="vds-h-6 vds-w-6 vds-text-dim/70 vds-hover:text-dim vds-flex-shrink-0"
          aria-label={revealed ? t('common.hide') : t('common.show')}
          onClick={handleReveal} disabled={isFetching}
          title={revealed ? t('common.hide') : t('common.show')}>
          {revealed
            ? <EyeOff className="vds-h-3.5 vds-w-3.5" />
            : <Eye className="vds-h-3.5 vds-w-3.5" />}
        </Button>
      )}
    </div>
  )
}

// ── Gemini model toggle with optimistic update ─────────────────────────────────

function GeminiModelToggle({ providerId, model }: { providerId: string; model: ProviderSelectedModel }) {
  const queryClient = useQueryClient()
  const [optimistic, setOptimistic] = useOptimistic(model.is_enabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (enabled: boolean) => api.setModelEnabled(providerId, model.model_name, enabled),
    onError: () => setOptimistic(model.is_enabled),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: [...GEMINI_QUERY_KEYS.selectedModels, providerId] })
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

export function ModelSelectionModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
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
            {t('providers.gemini.modelSelection')}
            <span className="vds-text-dim vds-font-400 vds-text-sm">— {provider.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="vds-text-xs vds-text-dim vds--mt-1">
          {t('providers.gemini.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="vds-flex vds-h-20 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="vds-text-sm vds-text-dim vds-py-4 vds-text-center">
            {t('providers.gemini.noGlobalModels')}
          </p>
        )}

        {models.length > 0 && (
          <div className="vds-space-y-1 vds-max-h-80 vds-overflow-y-auto vds-pr-1">
            {models.map((m) => (
              <div key={m.model_name}
                className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-3 vds-py-2">
                <span className="vds-font-mono vds-text-sm vds-text-bright">{m.model_name}</span>
                <GeminiModelToggle providerId={provider.id} model={m} />
              </div>
            ))}
          </div>
        )}

        {models.length > 0 && (
          <p className="vds-text-xs vds-text-dim vds-text-right">
            {t('providers.gemini.modelsCount', { enabled: enabledCount, total: models.length })}
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── SetSyncKeyModal ────────────────────────────────────────────────────────────

export function SetSyncKeyModal({ current, onClose }: { current: string | null; onClose: () => void }) {
  const { t } = useTranslation()
  const [apiKey, setApiKey] = useState('')
  const mutation = useApiMutation(
    () => api.setGeminiSyncConfig(apiKey.trim()),
    { invalidateKey: GEMINI_QUERY_KEYS.syncConfig, onSuccess: () => onClose() },
  )

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-sm">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <Key className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
            {t('providers.gemini.setSyncKey')}
          </DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-3">
          {current && (
            <p className="vds-text-xs vds-text-dim">
              {t('providers.gemini.syncKey')}: <span className="vds-font-mono vds-text-dim">{current}</span>
            </p>
          )}
          <div className="vds-space-y-1.5">
            <Label htmlFor="sync-key">{t('providers.gemini.syncKey')} <span className="vds-text-destructive">*</span></Label>
            <Input id="sync-key" type="password" value={apiKey}
              onChange={(e) => setApiKey(e.target.value)} placeholder={t('providers.gemini.apiKeyPlaceholder')} />
            <p className="vds-text-xs vds-text-dim">{t('providers.gemini.syncKeyHint')}</p>
          </div>
        </div>
        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}
        <DialogFooter className="vds-gap-3 vds-flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate(undefined)} disabled={!apiKey.trim() || mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
