'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Provider, GpuServer, RegisterProviderRequest, UpdateProviderRequest } from '@/lib/types'
import { serverMetricsQuery } from '@/lib/queries'
import { Server, Key } from 'lucide-react'
import { fmtMb, fmtTemp, fmtPower } from '@/lib/chart-theme'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI } from '@/lib/constants'
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
                          {(gpu.temp_junction_c ?? gpu.temp_c) != null ? ` — ${fmtTemp(gpu.temp_junction_c ?? gpu.temp_c)}` : ''}
                          {gpu.power_w != null ? ` · ${fmtPower(gpu.power_w)}` : ''}
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
                          {(gpu.temp_junction_c ?? gpu.temp_c) != null ? ` — ${fmtTemp(gpu.temp_junction_c ?? gpu.temp_c)}` : ''}
                          {gpu.power_w != null ? ` · ${fmtPower(gpu.power_w)}` : ''}
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
