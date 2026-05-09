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
import { PROVIDER_LLAMA_SERVER, PROVIDER_GEMINI } from '@/lib/constants'
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
      duplicate: t('providers.llama_server.duplicateUrl'),
      network: t('providers.llama_server.networkError'),
      unreachable: t('providers.llama_server.unreachableError'),
      fallback: t('providers.llama_server.connectionFailed'),
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

  const isLlamaServerUrlVerified = !urlChanged || (verifyState === 'ok' && url.trim() === verifiedUrl)

  const mutation = useMutation({
    mutationFn: () => {
      const body: UpdateProviderRequest = {
        name: name.trim(),
        url: provider.provider_type === PROVIDER_LLAMA_SERVER ? url.trim() : undefined,
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
            {provider.provider_type === PROVIDER_LLAMA_SERVER
              ? <><Server className="vds-h-4 vds-w-4 vds-text-info" /> {t('providers.llama_server.editTitle')}</>
              : <><Key className="vds-h-4 vds-w-4 vds-text-accent-gpu" /> {t('providers.gemini.editTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-name">{t('providers.llama_server.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="edit-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          {provider.provider_type === PROVIDER_LLAMA_SERVER && (
            <>
              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-url">{t('providers.llama_server.url')}</Label>
                <div className="vds-flex vds-gap-2">
                  <Input id="edit-url" type="url" value={url}
                    onChange={(e) => handleUrlChange(e.target.value)}
                    className={urlChanged ? (verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : '') : ''} />
                  {urlChanged && (
                    <Button type="button" variant="outline" size="sm" className="vds-flex-shrink-0"
                      disabled={!url.trim() || verifyState === 'checking'}
                      onClick={() => verify(url.trim())}>
                      {verifyState === 'checking' ? t('providers.llama_server.verifying')
                        : verifyState === 'ok' ? <><CheckCircle2 className="vds-h-3.5 vds-w-3.5 vds-mr-1 vds-text-success" />{t('providers.llama_server.connected')}</>
                        : t('providers.llama_server.verifyConnection')}
                    </Button>
                  )}
                </div>
                {verifyState === 'error' && <p className="vds-text-xs vds-text-destructive vds-flex vds-items-center vds-gap-1"><XCircle className="vds-h-3 vds-w-3" />{verifyError}</p>}
                {urlChanged && verifyState === 'idle' && <p className="vds-text-xs vds-text-dim">{t('providers.llama_server.verifyFirst')}</p>}
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-server">
                  {t('providers.llama_server.gpuServer')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="edit-server"><SelectValue placeholder={t('providers.llama_server.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('providers.llama_server.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="edit-gpu-index">{t('providers.llama_server.gpuIndex')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger aria-label={t('providers.llama_server.gpuIndex')}><SelectValue placeholder={t('providers.llama_server.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.llama_server.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          {t('providers.llama_server.gpuLabel')} {i} ({gpu.card})
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
                    placeholder={t('providers.llama_server.gpuIndexPlaceholder')} />
                )}
              </div>

              <div className="vds-space-y-1.5">
                <div className="vds-flex vds-items-center vds-justify-between">
                  <Label>{t('providers.llama_server.maxVram')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="vds-text-2xs vds-text-dim vds-tabular-nums">
                      {t('providers.llama_server.serverRam')}: <span className="vds-font-600 vds-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} aria-label={t('providers.llama_server.maxVram')} />
              </div>

              <div className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-border-subtle vds-px-4 vds-py-3">
                <div>
                  <p className="vds-text-sm vds-font-500">{t('providers.llama_server.freeTier')}</p>
                  <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('providers.llama_server.freeTierDesc')}</p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} aria-label={t('providers.llama_server.freeTier')} />
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
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || (provider.provider_type === PROVIDER_LLAMA_SERVER && !isLlamaServerUrlVerified) || mutation.isPending}>
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
  initialType: 'llama_server' | 'gemini'
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
      duplicate: t('providers.llama_server.duplicateUrl'),
      network: t('providers.llama_server.networkError'),
      unreachable: t('providers.llama_server.unreachableError'),
      fallback: t('providers.llama_server.connectionFailed'),
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
        ...(initialType === 'llama_server' && {
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

  const isLlamaServerVerified = verifyState === 'ok' && url.trim() === verifiedUrl
  const isValid = name.trim() && (
    initialType === 'llama_server' ? isLlamaServerVerified : apiKey.trim()
  )

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            {initialType === 'llama_server'
              ? <><Server className="vds-h-4 vds-w-4 vds-text-info" /> {t('providers.llama_server.registerTitle')}</>
              : <><Key className="vds-h-4 vds-w-4 vds-text-accent-gpu" /> {t('providers.gemini.registerTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="provider-name">{t('providers.llama_server.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="provider-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={initialType === 'llama_server' ? t('providers.llama_server.namePlaceholder') : t('providers.gemini.namePlaceholder')} />
          </div>

          {initialType === 'llama_server' && (
            <>
              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-url">{t('providers.llama_server.url')} <span className="vds-text-destructive">*</span></Label>
                <div className="vds-flex vds-gap-2">
                  <Input
                    id="provider-url"
                    type="url"
                    value={url}
                    onChange={(e) => handleUrlChange(e.target.value)}
                    placeholder={t('providers.llama_server.urlPlaceholder')}
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
                      ? t('providers.llama_server.verifying')
                      : t('providers.llama_server.verifyConnection')}
                  </Button>
                </div>
                {verifyState === 'ok' && (
                  <p className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-success">
                    <CheckCircle2 className="vds-h-3.5 vds-w-3.5" />
                    {t('providers.llama_server.connected')}
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
                  {t('providers.llama_server.gpuServer')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="provider-server"><SelectValue placeholder={t('providers.llama_server.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('providers.llama_server.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="vds-text-xs vds-text-dim">{t('providers.llama_server.gpuServerHint')}</p>
              </div>

              <div className="vds-space-y-1.5">
                <Label htmlFor="provider-gpu-index">{t('providers.llama_server.gpuIndex')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger aria-label={t('providers.llama_server.gpuIndex')}><SelectValue placeholder={t('providers.llama_server.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('providers.llama_server.noneOption')}</SelectItem>
                      {gpuCards.map((gpu, i) => (
                        <SelectItem key={gpu.card} value={String(i)}>
                          {t('providers.llama_server.gpuLabel')} {i} ({gpu.card})
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
                    placeholder={t('providers.llama_server.gpuIndexPlaceholder')} />
                )}
              </div>

              <div className="vds-space-y-1.5">
                <div className="vds-flex vds-items-center vds-justify-between">
                  <Label>{t('providers.llama_server.maxVram')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span></Label>
                  {serverMemTotalMb != null && serverMemTotalMb > 0 && (
                    <span className="vds-text-2xs vds-text-dim vds-tabular-nums">
                      {t('providers.llama_server.serverRam')}: <span className="vds-font-600 vds-text-dim">{fmtMb(serverMemTotalMb)}</span>
                    </span>
                  )}
                </div>
                <VramInput valueMb={vram} onChange={setVram} aria-label={t('providers.llama_server.maxVram')} />
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
            title={initialType === 'llama_server' && !isLlamaServerVerified ? t('providers.llama_server.verifyFirst') : undefined}
          >
            {mutation.isPending ? `${t('common.register')}…` : t('common.register')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
