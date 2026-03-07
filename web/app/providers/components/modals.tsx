'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Provider, ProviderSelectedModel, GeminiRateLimitPolicy, GpuServer, OllamaProviderForModel, RegisterProviderRequest, UpdateProviderRequest } from '@/lib/types'
import { serverMetricsQuery, selectedModelsQuery } from '@/lib/queries'
import { Server, Key, ShieldCheck, Eye, EyeOff, ListFilter, Search, Cpu, ChevronLeft, ChevronRight } from 'lucide-react'
import { ServerMetricsCompact, fmtMb } from '@/components/server-metrics-cell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { GEMINI_QUERY_KEYS } from '@/lib/queries/providers'
import {
  PROVIDER_OLLAMA, PROVIDER_GEMINI,
  PROVIDER_STATUS_DOT, PROVIDER_STATUS_BADGE, PROVIDER_STATUS_I18N,
} from '@/lib/constants'
import { extractHost, VramInput } from './shared'

// ── Edit provider modal ─────────────────────────────────────────────────────────

export function EditModal({ provider, servers, onClose }: { provider: Provider; servers: GpuServer[]; onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(provider.name)
  const [url, setUrl] = useState(provider.url)
  const [apiKey, setApiKey] = useState('')
  const [vram, setVram] = useState(provider.total_vram_mb > 0 ? String(provider.total_vram_mb) : '')
  const [gpuIndex, setGpuIndex] = useState(provider.gpu_index !== null ? String(provider.gpu_index) : 'none')
  const [serverId, setServerId] = useState<string>(provider.server_id ?? 'none')
  const [isFreeTier, setIsFreeTier] = useState(provider.is_free_tier)

  const { data: serverMetrics } = useQuery({
    ...serverMetricsQuery(serverId),
    enabled: serverId !== 'none',
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const serverMemTotalMb = serverMetrics?.mem_total_mb ?? null
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: UpdateProviderRequest = {
        name: name.trim(),
        url: provider.provider_type === PROVIDER_OLLAMA ? url.trim() : undefined,
        api_key: apiKey.trim() || undefined,
        total_vram_mb: vram ? parseInt(vram, 10) : 0,
        gpu_index: gpuIndex !== 'none' && gpuIndex !== '' ? parseInt(gpuIndex, 10) : null,
        server_id: serverId !== 'none' ? serverId : null,
        is_free_tier: isFreeTier,
      }
      return api.updateProvider(provider.id, body)
    },
    onSettled: () => { queryClient.invalidateQueries({ queryKey: ['providers'] }); onClose() },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {provider.provider_type === PROVIDER_OLLAMA
              ? <><Server className="h-4 w-4 text-status-info-fg" /> {t('providers.ollama.editTitle')}</>
              : <><Key className="h-4 w-4 text-accent-gpu" /> {t('providers.gemini.editTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-name">{t('providers.ollama.name')} <span className="text-destructive">*</span></Label>
            <Input id="edit-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          {provider.provider_type === PROVIDER_OLLAMA && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="edit-url">{t('providers.ollama.ollamaUrl')}</Label>
                <Input id="edit-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="edit-server">
                  {t('providers.ollama.gpuServer')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="edit-server"><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-1.5">
                <Label>{t('providers.ollama.gpuIndex')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          GPU {i} ({gpu.card})
                          {gpu.temp_c != null ? ` — ${gpu.temp_c.toFixed(0)}°C` : ''}
                          {gpu.power_w != null ? ` · ${gpu.power_w.toFixed(0)}W` : ''}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input type="number" min={0}
                    value={gpuIndex === 'none' ? '' : gpuIndex}
                    onChange={(e) => setGpuIndex(e.target.value)}
                    placeholder="0" />
                )}
              </div>

              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <Label>{t('providers.ollama.maxVram')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="text-[11px] text-muted-foreground tabular-nums">
                      {t('providers.ollama.serverRam')}: <span className="font-semibold text-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{t('providers.ollama.freeTier')}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('providers.ollama.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} />
              </div>
            </>
          )}

          {provider.provider_type === PROVIDER_GEMINI && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="edit-apikey">
                  {t('providers.gemini.apiKey')} <span className="text-muted-foreground font-normal">— {t('providers.gemini.keepExistingKey')}</span>
                </Label>
                <Input id="edit-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
                <p className="text-xs text-muted-foreground">{t('providers.gemini.apiKeyHint')}</p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{t('providers.gemini.freeTier')}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('providers.gemini.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Register provider modal ─────────────────────────────────────────────────────

export function RegisterModal({
  servers,
  initialType,
  onClose,
}: {
  servers: GpuServer[]
  initialType: 'ollama' | 'gemini'
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [vram, setVram] = useState('')
  const [gpuIndex, setGpuIndex] = useState('none')
  const [serverId, setServerId] = useState<string>('none')
  const [isFreeTier, setIsFreeTier] = useState(false)

  const { data: serverMetrics } = useQuery({
    ...serverMetricsQuery(serverId),
    enabled: serverId !== 'none',
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const serverMemTotalMb = serverMetrics?.mem_total_mb ?? null
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: RegisterProviderRequest = {
        name: name.trim(),
        provider_type: initialType,
        ...(initialType === 'ollama' && {
          url: url.trim(),
          total_vram_mb: vram ? parseInt(vram, 10) : undefined,
          gpu_index: gpuIndex !== 'none' && gpuIndex !== '' ? parseInt(gpuIndex, 10) : undefined,
          server_id: serverId !== 'none' ? serverId : undefined,
        }),
        ...(initialType === 'gemini' && {
          api_key: apiKey.trim(),
          is_free_tier: isFreeTier,
        }),
      }
      return api.registerProvider(body)
    },
    onSettled: () => { queryClient.invalidateQueries({ queryKey: ['providers'] }); onClose() },
  })

  const isValid = name.trim() && (initialType === 'ollama' ? url.trim() : apiKey.trim())

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {initialType === 'ollama'
              ? <><Server className="h-4 w-4 text-status-info-fg" /> {t('providers.ollama.registerTitle')}</>
              : <><Key className="h-4 w-4 text-accent-gpu" /> {t('providers.gemini.registerTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="provider-name">{t('providers.ollama.name')} <span className="text-destructive">*</span></Label>
            <Input id="provider-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={initialType === 'ollama' ? 'e.g. gpu-server-1' : 'e.g. gemini-prod'} />
          </div>

          {initialType === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="provider-url">{t('providers.ollama.ollamaUrl')} <span className="text-destructive">*</span></Label>
                <Input id="provider-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://192.168.1.10:11434" />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="provider-server">
                  {t('providers.ollama.gpuServer')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="provider-server"><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">{t('providers.ollama.gpuServerHint')}</p>
              </div>

              <div className="space-y-1.5">
                <Label>{t('providers.ollama.gpuIndex')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          GPU {i} ({gpu.card})
                          {gpu.temp_c != null ? ` — ${gpu.temp_c.toFixed(0)}°C` : ''}
                          {gpu.power_w != null ? ` · ${gpu.power_w.toFixed(0)}W` : ''}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input type="number" min={0}
                    value={gpuIndex === 'none' ? '' : gpuIndex}
                    onChange={(e) => setGpuIndex(e.target.value)}
                    placeholder="0" />
                )}
              </div>

              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <Label>{t('providers.ollama.maxVram')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="text-[11px] text-muted-foreground tabular-nums">
                      {t('providers.ollama.serverRam')}: <span className="font-semibold text-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>
            </>
          )}

          {initialType === 'gemini' && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="provider-apikey">{t('providers.gemini.apiKey')} <span className="text-destructive">*</span></Label>
                <Input id="provider-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
                <p className="text-xs text-muted-foreground">{t('providers.gemini.apiKeyHint')}</p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{t('providers.gemini.freeTier')}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('providers.gemini.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!isValid || mutation.isPending}>
            {mutation.isPending ? `${t('common.register')}…` : t('common.register')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Gemini rate limit policy modal ─────────────────────────────────────────────

export function EditPolicyModal({ policy, onClose }: { policy: GeminiRateLimitPolicy; onClose: () => void }) {
  const { t } = useTranslation()
  const [rpm, setRpm] = useState(String(policy.rpm_limit))
  const [rpd, setRpd] = useState(String(policy.rpd_limit))
  const [availableOnFreeTier, setAvailableOnFreeTier] = useState(policy.available_on_free_tier)
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () =>
      api.upsertGeminiPolicy(policy.model_name, {
        rpm_limit: rpm ? parseInt(rpm, 10) : 0,
        rpd_limit: rpd ? parseInt(rpd, 10) : 0,
        available_on_free_tier: availableOnFreeTier,
      }),
    onSettled: () => { queryClient.invalidateQueries({ queryKey: ['gemini-policies'] }); onClose() },
  })

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
            <Switch checked={availableOnFreeTier} onCheckedChange={setAvailableOnFreeTier} />
          </div>

          {availableOnFreeTier && (
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="pol-rpm" className="text-xs">RPM <span className="text-muted-foreground font-normal">(req/min)</span></Label>
                <Input id="pol-rpm" type="number" min={0} value={rpm}
                  onChange={(e) => setRpm(e.target.value)} placeholder="e.g. 10" className="h-8 text-sm" />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="pol-rpd" className="text-xs">RPD <span className="text-muted-foreground font-normal">(req/day)</span></Label>
                <Input id="pol-rpd" type="number" min={0} value={rpd}
                  onChange={(e) => setRpd(e.target.value)} placeholder="e.g. 250" className="h-8 text-sm" />
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
        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
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

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['provider-key', providerId],
    queryFn: () => api.providerKey(providerId),
    enabled: false,
  })

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

// ── Model selection modal ──────────────────────────────────────────────────────

export function ModelSelectionModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
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
      queryClient.invalidateQueries({ queryKey: [...GEMINI_QUERY_KEYS.selectedModels, provider.id] })
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
                <Switch
                  checked={m.is_enabled}
                  onCheckedChange={(checked) =>
                    toggleMutation.mutate({ modelName: m.model_name, isEnabled: checked })
                  }
                  disabled={toggleMutation.isPending}
                />
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
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => api.setGeminiSyncConfig(apiKey.trim()),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.syncConfig })
      onClose()
    },
  })

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
              onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
            <p className="text-xs text-muted-foreground">{t('providers.gemini.syncKeyHint')}</p>
          </div>
        </div>
        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}
        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!apiKey.trim() || mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── OllamaModelProvidersModal ───────────────────────────────────────────────────

const PROVIDERS_PAGE_SIZE = 8

export function OllamaModelProvidersModal({ modelName, onClose }: { modelName: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)

  const { data, isLoading } = useQuery<{ providers: OllamaProviderForModel[] }>({
    queryKey: ['ollama-model-providers', modelName],
    queryFn: () => api.ollamaModelProviders(modelName),
    staleTime: 30_000,
  })

  const allProviders = data?.providers ?? []
  const filtered = allProviders.filter((b) =>
    b.name.toLowerCase().includes(search.toLowerCase()) ||
    b.url.toLowerCase().includes(search.toLowerCase())
  )

  const totalPages = Math.max(1, Math.ceil(filtered.length / PROVIDERS_PAGE_SIZE))
  const safePage = Math.min(page, totalPages)
  const pageStart = (safePage - 1) * PROVIDERS_PAGE_SIZE
  const pageItems = filtered.slice(pageStart, pageStart + PROVIDERS_PAGE_SIZE)

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
                <Badge variant="outline" className={statusBadgeCls(b.status)}>
                  {statusLabel(b.status)}
                </Badge>
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
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={safePage <= 1}>
                <ChevronLeft className="h-3.5 w-3.5" />
              </Button>
              <span className="text-xs text-muted-foreground px-1">
                {safePage} / {totalPages}
              </span>
              <Button variant="outline" size="icon" className="h-7 w-7"
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
