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
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-accent-gpu" />
            {t('providers.gemini.editPolicyTitle')}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-1 mb-1">
          <p className="text-sm text-muted-foreground">{t('providers.gemini.model')}</p>
          <p className="font-mono text-sm font-semibold text-text-bright">
            {policy.model_name === '*' ? `* (${t('providers.gemini.globalDefault')})` : policy.model_name}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
            <div>
              <p className="text-sm font-medium">{t('providers.gemini.availableOnFreeTier')}</p>
              <p className="text-xs text-muted-foreground mt-0.5">
                {availableOnFreeTier
                  ? t('providers.gemini.freeTierRouting')
                  : t('providers.gemini.paidOnlyRouting')}
              </p>
            </div>
            <Switch checked={availableOnFreeTier} onCheckedChange={setAvailableOnFreeTier} aria-label={t('providers.gemini.availableOnFreeTier')} />
          </div>

          {availableOnFreeTier && (
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="pol-rpm" className="text-xs">{t('providers.gemini.rpm')} <span className="text-muted-foreground font-normal">({t('providers.gemini.rpmUnit')})</span></Label>
                <Input id="pol-rpm" type="number" min={0} value={rpm}
                  onChange={(e) => setRpm(e.target.value)} placeholder={t('providers.gemini.rpmPlaceholder')} className="h-8 text-sm" />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="pol-rpd" className="text-xs">{t('providers.gemini.rpd')} <span className="text-muted-foreground font-normal">({t('providers.gemini.rpdUnit')})</span></Label>
                <Input id="pol-rpd" type="number" min={0} value={rpd}
                  onChange={(e) => setRpd(e.target.value)} placeholder={t('providers.gemini.rpdPlaceholder')} className="h-8 text-sm" />
              </div>
              <p className="col-span-2 text-[11px] text-muted-foreground -mt-1">
                {t('providers.gemini.freeLimitsHint')}
              </p>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('providers.gemini.failedToSave')}
          </p>
        )}
        <DialogFooter className="gap-3 flex-wrap">
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
    <div className="flex items-center gap-1.5">
      <span className="font-mono text-xs text-muted-foreground select-all">{displayKey}</span>
      {masked && (
        <Button variant="ghost" size="icon"
          className="h-6 w-6 text-muted-foreground/70 hover:text-text-dim shrink-0"
          aria-label={revealed ? t('common.hide') : t('common.show')}
          onClick={handleReveal} disabled={isFetching}
          title={revealed ? t('common.hide') : t('common.show')}>
          {revealed
            ? <EyeOff className="h-3.5 w-3.5" />
            : <Eye className="h-3.5 w-3.5" />}
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
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ListFilter className="h-4 w-4 text-accent-gpu" />
            {t('providers.gemini.modelSelection')}
            <span className="text-muted-foreground font-normal text-sm">— {provider.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="text-xs text-muted-foreground -mt-1">
          {t('providers.gemini.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="flex h-20 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center">
            {t('providers.gemini.noGlobalModels')}
          </p>
        )}

        {models.length > 0 && (
          <div className="space-y-1 max-h-80 overflow-y-auto pr-1">
            {models.map((m) => (
              <div key={m.model_name}
                className="flex items-center justify-between rounded-lg border border-border px-3 py-2">
                <span className="font-mono text-sm text-text-bright">{m.model_name}</span>
                <GeminiModelToggle providerId={provider.id} model={m} />
              </div>
            ))}
          </div>
        )}

        {models.length > 0 && (
          <p className="text-xs text-muted-foreground text-right">
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
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Key className="h-4 w-4 text-accent-gpu" />
            {t('providers.gemini.setSyncKey')}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          {current && (
            <p className="text-xs text-muted-foreground">
              {t('providers.gemini.syncKey')}: <span className="font-mono text-text-dim">{current}</span>
            </p>
          )}
          <div className="space-y-1.5">
            <Label htmlFor="sync-key">{t('providers.gemini.syncKey')} <span className="text-destructive">*</span></Label>
            <Input id="sync-key" type="password" value={apiKey}
              onChange={(e) => setApiKey(e.target.value)} placeholder={t('providers.gemini.apiKeyPlaceholder')} />
            <p className="text-xs text-muted-foreground">{t('providers.gemini.syncKeyHint')}</p>
          </div>
        </div>
        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}
        <DialogFooter className="gap-3 flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate(undefined)} disabled={!apiKey.trim() || mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
