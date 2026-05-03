'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Provider, GpuServer, RegisterProviderRequest, UpdateProviderRequest } from '@/lib/types'
import { useVerifyUrl } from '@/hooks/use-verify-url'
import { serverMetricsQuery } from '@/lib/queries'
import { Server, Key, CheckCircle2, XCircle } from 'lucide-react'
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
  const { verifyState, verifyError, verifiedUrl, verify, handleUrlChange: onVerifyReset } = useVerifyUrl({
    verifyFn: api.verifyProvider,
    labels: {
      duplicate: t('providers.ollama.duplicateUrl'),
      network: t('providers.ollama.networkError'),
      unreachable: t('providers.ollama.unreachableError'),
      fallback: t('providers.ollama.connectionFailed'),
    },
    initialUrl: provider.url,
  })

  const urlChanged = url.trim() !== provider.url

  const handleUrlChange = (val: string) => { setUrl(val); onVerifyReset() }

  const { data: serverMetrics } = useQuery({
    ...serverMetricsQuery(serverId),
    enabled: serverId !== 'none',
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const serverMemTotalMb = serverMetrics?.mem_total_mb ?? null
  const queryClient = useQueryClient()

  const isOllamaUrlVerified = !urlChanged || (verifyState === 'ok' && url.trim() === verifiedUrl)

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
    onSuccess: () => onClose(),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            {provider.provider_type === PROVIDER_OLLAMA
              ? <><Server className="vds-h-4 vds-w-4 vds-text-info" /> {t('providers.ollama.editTitle')}</>
              : <><Key className="vds-h-4 vds-w-4 vds-text-accent-gpu" /> {t('providers.gemini.editTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-name">{t('providers.ollama.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="edit-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          {provider.provider_type === PROVIDER_OLLAMA && (
            <>
              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-url">{t('providers.ollama.ollamaUrl')}</Label>
                <div className="vds-flex vds-gap-2">
                  <Input id="edit-url" type="url" value={url}
                    onChange={(e) => handleUrlChange(e.target.value)}
                    className={urlChanged ? (verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : '') : ''} />
                  {urlChanged && (
                    <Button type="button" variant="outline" size="sm" className="vds-flex-shrink-0"
                      disabled={!url.trim() || verifyState === 'checking'}
                      onClick={() => verify(url.trim())}>
                      {verifyState === 'checking' ? t('providers.ollama.verifying')
                        : verifyState === 'ok' ? <><CheckCircle2 className="vds-h-3.5 vds-w-3.5 vds-mr-1 vds-text-success" />{t('providers.ollama.connected')}</>
                        : t('providers.ollama.verifyConnection')}
                    </Button>
                  )}
                </div>
                {verifyState === 'error' && <p className="vds-text-xs vds-text-destructive vds-flex vds-items-center vds-gap-1"><XCircle className="vds-h-3 vds-w-3" />{verifyError}</p>}
                {urlChanged && verifyState === 'idle' && <p className="vds-text-xs vds-text-dim">{t('providers.ollama.verifyFirst')}</p>}
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-server">
                  {t('providers.ollama.gpuServer')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span>
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

              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-gpu-index">{t('providers.ollama.gpuIndex')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger aria-label={t('providers.ollama.gpuIndex')}><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          {t('providers.ollama.gpuLabel')} {i} ({gpu.card})
                          {(gpu.temp_junction_c ?? gpu.temp_c) != null ? ` — ${fmtTemp(gpu.temp_junction_c ?? gpu.temp_c)}` : ''}
                          {gpu.power_w != null ? ` · ${fmtPower(gpu.power_w)}` : ''}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input id="edit-gpu-index" type="number" min={0}
                    value={gpuIndex === 'none' ? '' : gpuIndex}
                    onChange={(e) => setGpuIndex(e.target.value)}
                    placeholder={t('providers.ollama.gpuIndexPlaceholder')} />
                )}
              </div>

              <div className="vds-space-y-1.5">
                <div className="vds-flex vds-items-center vds-justify-between">
                  <Label>{t('providers.ollama.maxVram')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="vds-text-2xs vds-text-dim vds-tabular-nums">
                      {t('providers.ollama.serverRam')}: <span className="vds-font-600 vds-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} aria-label={t('providers.ollama.maxVram')} />
              </div>

              <div className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-4 vds-py-3">
                <div>
                  <p className="vds-text-sm vds-font-500">{t('providers.ollama.freeTier')}</p>
                  <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('providers.ollama.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} aria-label={t('providers.ollama.freeTier')} />
              </div>
            </>
          )}

          {provider.provider_type === PROVIDER_GEMINI && (
            <div className="vds-space-y-4">
              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-apikey">
                  {t('providers.gemini.apiKey')} <span className="vds-text-dim vds-font-400">— {t('providers.gemini.keepExistingKey')}</span>
                </Label>
                <Input id="edit-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder={t('providers.gemini.apiKeyPlaceholder')} />
                <p className="vds-text-xs vds-text-dim">{t('providers.gemini.apiKeyHint')}</p>
              </div>

              <div className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-4 vds-py-3">
                <div>
                  <p className="vds-text-sm vds-font-500">{t('providers.gemini.freeTier')}</p>
                  <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('providers.gemini.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} aria-label={t('providers.gemini.freeTier')} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || (provider.provider_type === PROVIDER_OLLAMA && !isOllamaUrlVerified) || mutation.isPending}>
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

  const { verifyState, verifyError, verifiedUrl, verify, handleUrlChange: onVerifyReset } = useVerifyUrl({
    verifyFn: api.verifyProvider,
    labels: {
      duplicate: t('providers.ollama.duplicateUrl'),
      network: t('providers.ollama.networkError'),
      unreachable: t('providers.ollama.unreachableError'),
      fallback: t('providers.ollama.connectionFailed'),
    },
  })

  const { data: serverMetrics } = useQuery({
    ...serverMetricsQuery(serverId),
    enabled: serverId !== 'none',
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const serverMemTotalMb = serverMetrics?.mem_total_mb ?? null
  const queryClient = useQueryClient()

  const handleUrlChange = (val: string) => { setUrl(val); onVerifyReset() }

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

  const isOllamaVerified = verifyState === 'ok' && url.trim() === verifiedUrl
  const isValid = name.trim() && (
    initialType === 'ollama' ? isOllamaVerified : apiKey.trim()
  )

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            {initialType === 'ollama'
              ? <><Server className="vds-h-4 vds-w-4 vds-text-info" /> {t('providers.ollama.registerTitle')}</>
              : <><Key className="vds-h-4 vds-w-4 vds-text-accent-gpu" /> {t('providers.gemini.registerTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="provider-name">{t('providers.ollama.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="provider-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={initialType === 'ollama' ? t('providers.ollama.namePlaceholder') : t('providers.gemini.namePlaceholder')} />
          </div>

          {initialType === 'ollama' && (
            <>
              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-url">{t('providers.ollama.ollamaUrl')} <span className="vds-text-destructive">*</span></Label>
                <div className="vds-flex vds-gap-2">
                  <Input
                    id="provider-url"
                    type="url"
                    value={url}
                    onChange={(e) => handleUrlChange(e.target.value)}
                    placeholder={t('providers.ollama.urlPlaceholder')}
                    className={verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : ''}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="vds-flex-shrink-0"
                    disabled={!url.trim() || verifyState === 'checking'}
                    onClick={() => verify(url.trim())}
                  >
                    {verifyState === 'checking'
                      ? t('providers.ollama.verifying')
                      : t('providers.ollama.verifyConnection')}
                  </Button>
                </div>
                {verifyState === 'ok' && (
                  <p className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-success">
                    <CheckCircle2 className="vds-h-3.5 vds-w-3.5" />
                    {t('providers.ollama.connected')}
                  </p>
                )}
                {verifyState === 'error' && (
                  <p className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-destructive">
                    <XCircle className="vds-h-3.5 vds-w-3.5" />
                    {verifyError}
                  </p>
                )}
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-server">
                  {t('providers.ollama.gpuServer')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span>
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
                <p className="vds-text-xs vds-text-dim">{t('providers.ollama.gpuServerHint')}</p>
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-gpu-index">{t('providers.ollama.gpuIndex')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger aria-label={t('providers.ollama.gpuIndex')}><SelectValue placeholder={t('providers.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.ollama.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          {t('providers.ollama.gpuLabel')} {i} ({gpu.card})
                          {(gpu.temp_junction_c ?? gpu.temp_c) != null ? ` — ${fmtTemp(gpu.temp_junction_c ?? gpu.temp_c)}` : ''}
                          {gpu.power_w != null ? ` · ${fmtPower(gpu.power_w)}` : ''}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input id="provider-gpu-index" type="number" min={0}
                    value={gpuIndex === 'none' ? '' : gpuIndex}
                    onChange={(e) => setGpuIndex(e.target.value)}
                    placeholder={t('providers.ollama.gpuIndexPlaceholder')} />
                )}
              </div>

              <div className="vds-space-y-1.5">
                <div className="vds-flex vds-items-center vds-justify-between">
                  <Label>{t('providers.ollama.maxVram')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="vds-text-2xs vds-text-dim vds-tabular-nums">
                      {t('providers.ollama.serverRam')}: <span className="vds-font-600 vds-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} aria-label={t('providers.ollama.maxVram')} />
              </div>
            </>
          )}

          {initialType === 'gemini' && (
            <div className="vds-space-y-4">
              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-apikey">{t('providers.gemini.apiKey')} <span className="vds-text-destructive">*</span></Label>
                <Input id="provider-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder={t('providers.gemini.apiKeyPlaceholder')} />
                <p className="vds-text-xs vds-text-dim">{t('providers.gemini.apiKeyHint')}</p>
              </div>

              <div className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-4 vds-py-3">
                <div>
                  <p className="vds-text-sm vds-font-500">{t('providers.gemini.freeTier')}</p>
                  <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('providers.gemini.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} aria-label={t('providers.gemini.freeTier')} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!isValid || mutation.isPending}
            title={initialType === 'ollama' && !isOllamaVerified ? t('providers.ollama.verifyFirst') : undefined}
          >
            {mutation.isPending ? `${t('common.register')}…` : t('common.register')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
