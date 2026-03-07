'use client'

import { useState, useRef, Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { providersQuery, serversQuery, serverMetricsQuery, selectedModelsQuery, ollamaModelsQuery, ollamaSyncStatusQuery, geminiPoliciesQuery, geminiModelsQuery, geminiSyncConfigQuery, capacityQuery, syncSettingsQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Provider, ProviderVramInfo, ProviderSelectedModel, SyncSettings, GeminiModel, GeminiRateLimitPolicy, GeminiStatusResult, GeminiStatusSyncResponse, GeminiSyncConfig, GpuServer, LoadedModelInfo, NodeMetrics, OllamaProviderForModel, OllamaModelWithCount, OllamaSyncJob, PatchSyncSettings, RegisterProviderRequest, UpdateProviderRequest } from '@/lib/types'
import { Activity, AlertTriangle, Plus, Trash2, RefreshCw, RotateCcw, Server, Key, Wifi, WifiOff, AlertCircle, Pencil, ShieldCheck, Eye, EyeOff, ListFilter, Search, BarChart2, Cpu, ChevronLeft, ChevronRight } from 'lucide-react'
import { ServerMetricsCompact, fmtMb } from '@/components/server-metrics-cell'
import { ServerHistoryModal } from '@/components/server-history-modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { useLabSettings } from '@/components/lab-settings-provider'
import { fmtDateOnly, fmtDatetimeShort } from '@/lib/date'

// SSOT: Gemini query keys imported from central definitions.
import { GEMINI_QUERY_KEYS } from '@/lib/queries/providers'
import {
  PROVIDER_OLLAMA, PROVIDER_GEMINI,
  PROVIDER_STATUS_DOT, PROVIDER_STATUS_DOT_ALT,
  PROVIDER_STATUS_BADGE, PROVIDER_STATUS_TEXT, PROVIDER_STATUS_I18N,
} from '@/lib/constants'

// ── Helpers ────────────────────────────────────────────────────────────────────

function extractHost(url: string): string {
  try { return new URL(url).host } catch { return url }
}

// ── Status badge ───────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: Provider['status'] }) {
  const { t } = useTranslation()
  if (status === 'online') return (
    <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 font-medium">
      <Wifi className="h-3 w-3 mr-1.5" />{t('common.online')}
    </Badge>
  )
  if (status === 'degraded') return (
    <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 font-medium">
      <AlertCircle className="h-3 w-3 mr-1.5" />{t('common.degraded')}
    </Badge>
  )
  return (
    <Badge variant="outline" className="bg-surface-code text-muted-foreground border-border font-medium">
      <WifiOff className="h-3 w-3 mr-1.5" />{t('common.offline')}
    </Badge>
  )
}


// ── VRAM input with MiB / GiB toggle ──────────────────────────────────────────

function VramInput({ valueMb, onChange }: { valueMb: string; onChange: (mb: string) => void }) {
  const [unit, setUnit] = useState<'mb' | 'gb'>('mb')
  const mbNum = parseInt(valueMb) || 0
  const display = mbNum > 0
    ? (unit === 'gb' ? String(Math.round(mbNum / 1024 * 10) / 10) : String(mbNum))
    : ''

  function handleInput(raw: string) {
    if (!raw) { onChange(''); return }
    const n = parseFloat(raw)
    if (isNaN(n) || n < 0) return
    onChange(String(Math.round(unit === 'gb' ? n * 1024 : n)))
  }

  return (
    <div className="flex">
      <Input type="number" min={0} step={unit === 'gb' ? 0.5 : 256}
        value={display} onChange={(e) => handleInput(e.target.value)}
        placeholder={unit === 'gb' ? 'e.g. 24' : 'e.g. 24576'}
        className="rounded-r-none" />
      <Button type="button" variant={unit === 'mb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('mb')}
        className="h-9 px-2 text-xs rounded-none border-l-0 border-r-0 shrink-0">MiB</Button>
      <Button type="button" variant={unit === 'gb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('gb')}
        className="h-9 px-2 text-xs rounded-l-none shrink-0">GiB</Button>
    </div>
  )
}

// ── Edit provider modal ─────────────────────────────────────────────────────────

function EditModal({ provider, servers, onClose }: { provider: Provider; servers: GpuServer[]; onClose: () => void }) {
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

function RegisterModal({
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

function EditPolicyModal({ policy, onClose }: { policy: GeminiRateLimitPolicy; onClose: () => void }) {
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

function ApiKeyCell({ providerId, masked }: { providerId: string; masked: string | null }) {
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

function ModelSelectionModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
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

function SetSyncKeyModal({ current, onClose }: { current: string | null; onClose: () => void }) {
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

function OllamaModelProvidersModal({ modelName, onClose }: { modelName: string; onClose: () => void }) {
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

function OllamaProviderModelsModal({ provider, onClose }: { provider: Provider; onClose: () => void }) {
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

// ── Shared page size ───────────────────────────────────────────────────────────

const PAGE_SIZE = 10

// ── Ollama Global Sync Section ─────────────────────────────────────────────────

function OllamaSyncSection() {
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
  const filteredModels = allModels.filter((m) =>
    m.model_name.toLowerCase().includes(search.toLowerCase())
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

// ── Ollama Capacity Section ────────────────────────────────────────────────────

function ThermalBadge({ state }: { state: 'normal' | 'soft' | 'hard' }) {
  const { t } = useTranslation()
  if (state === 'hard') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-error/15 text-status-error-fg border border-status-error/30">
      <AlertTriangle className="h-2.5 w-2.5" />{t('providers.capacity.thermal.hard')}
    </span>
  )
  if (state === 'soft') return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-warn/15 text-status-warn-fg border border-status-warn/30">
      <AlertTriangle className="h-2.5 w-2.5" />{t('providers.capacity.thermal.soft')}
    </span>
  )
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-status-success/10 text-status-success-fg border border-status-success/30">
      <span className="h-1.5 w-1.5 rounded-full bg-status-success" />{t('providers.capacity.thermal.normal')}
    </span>
  )
}

import { fmtMbShort } from '@/lib/chart-theme'

function VramBar({ used, total }: { used: number; total: number }) {
  if (total === 0) return <span className="text-xs text-muted-foreground italic">unknown</span>
  const pct = Math.min(100, Math.round((used / total) * 100))
  const color = pct > 90 ? 'bg-status-error' : pct > 70 ? 'bg-status-warn' : 'bg-status-success'
  return (
    <div className="flex items-center gap-2 min-w-32">
      <div className="flex-1 h-2 rounded-full bg-muted/60 overflow-hidden">
        <div className={`h-full rounded-full ${color} transition-all`} style={{ width: `${pct}%` }} />
      </div>
      <span className="text-xs text-muted-foreground tabular-nums shrink-0">{pct}%</span>
    </div>
  )
}

function OllamaCapacitySection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const { data: capacityData, isLoading: capacityLoading } = useQuery(capacityQuery)
  const { data: settings } = useQuery(syncSettingsQuery)

  const [analyzerModel, setAnalyzerModel] = useState<string>('')
  const [syncEnabled, setSyncEnabled] = useState<boolean>(true)
  const [intervalSecs, setIntervalSecs] = useState<string>('')
  const [probePermits, setProbePermits] = useState<string>('1')
  const [probeRate, setProbeRate] = useState<string>('3')

  // Sync local form state when settings load
  const prevSettingsRef = useRef<typeof settings>(null)
  if (settings && prevSettingsRef.current !== settings) {
    prevSettingsRef.current = settings
    setAnalyzerModel(settings.analyzer_model)
    setSyncEnabled(settings.sync_enabled)
    setIntervalSecs(String(settings.sync_interval_secs))
    setProbePermits(String(settings.probe_permits))
    setProbeRate(String(settings.probe_rate))
  }

  const saveMutation = useMutation({
    mutationFn: (body: PatchSyncSettings) => api.patchSyncSettings(body),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['sync-settings'] })
    },
  })

  const syncMutation = useMutation({
    mutationFn: () => api.syncAllProviders(),
    onSettled: () => {
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ['capacity'] })
        queryClient.invalidateQueries({ queryKey: ['sync-settings'] })
        queryClient.invalidateQueries({ queryKey: ['providers'] })
        queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
      }, 3000)
    },
  })

  const handleSave = () => {
    const body: PatchSyncSettings = {
      analyzer_model: analyzerModel || undefined,
      sync_enabled: syncEnabled,
      sync_interval_secs: intervalSecs ? Number(intervalSecs) : undefined,
      probe_permits: probePermits !== '' ? Number(probePermits) : undefined,
      probe_rate: probeRate !== '' ? Number(probeRate) : undefined,
    }
    saveMutation.mutate(body)
  }

  const providers = capacityData?.providers ?? []
  const lastRunAt = settings?.last_run_at
  const lastRunStatus = settings?.last_run_status
  const availableModels = settings?.available_models ?? []

  function fmtRelativeTime(iso: string | null) {
    if (!iso) return t('providers.capacity.never')
    const diff = Date.now() - new Date(iso).getTime()
    const mins = Math.floor(diff / 60_000)
    if (mins < 1) return '< 1 min ago'
    if (mins < 60) return `${mins} min ago`
    return `${Math.floor(mins / 60)}h ago`
  }

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <Activity className="h-4 w-4 text-accent-gpu" />
        {t('providers.capacity.title')}
      </h2>

      {/* Settings card */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <p className="text-sm font-medium text-text-bright">{t('providers.capacity.settings')}</p>
            <div className="flex items-center gap-2">
              {lastRunAt && (
                <span className="text-xs text-muted-foreground">
                  {t('providers.capacity.lastRun')}: {fmtRelativeTime(lastRunAt)}
                  {lastRunStatus && (
                    <span className={`ml-1.5 font-medium ${lastRunStatus === 'ok' ? 'text-status-success-fg' : 'text-status-error-fg'}`}>
                      · {lastRunStatus === 'ok' ? t('providers.capacity.statusOk') : t('providers.capacity.statusError')}
                    </span>
                  )}
                </span>
              )}
              <Button
                size="sm"
                variant="outline"
                onClick={() => syncMutation.mutate()}
                disabled={syncMutation.isPending}
                className="gap-1.5 shrink-0"
              >
                <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
                {syncMutation.isPending ? t('providers.capacity.syncing') : t('providers.capacity.syncNow')}
              </Button>
              {syncMutation.isSuccess && !syncMutation.isPending && (
                <span className="text-xs text-status-success-fg">✓ {t('providers.capacity.triggered')}</span>
              )}
            </div>
          </div>

          <div className="flex items-end gap-3 flex-wrap">
            <div className="space-y-1 min-w-44">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.analyzerModel')}</Label>
              <Select value={analyzerModel} onValueChange={setAnalyzerModel}>
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={analyzerModel || '—'} />
                </SelectTrigger>
                <SelectContent>
                  {availableModels.map((m) => (
                    <SelectItem key={m} value={m}>{m}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">{t('providers.capacity.interval')}</Label>
              <Input
                type="number"
                min={60}
                className="h-8 text-sm w-24"
                value={intervalSecs}
                onChange={(e) => setIntervalSecs(e.target.value)}
                disabled={!syncEnabled}
              />
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Probe Permits</Label>
              <Input
                type="number"
                className="h-8 text-sm w-20"
                value={probePermits}
                onChange={(e) => setProbePermits(e.target.value)}
              />
            </div>

            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Probe Rate</Label>
              <Input
                type="number"
                min={0}
                className="h-8 text-sm w-20"
                value={probeRate}
                onChange={(e) => setProbeRate(e.target.value)}
              />
            </div>

            <div className="flex items-center gap-2 pb-0.5">
              <Switch
                id="cap-auto"
                checked={syncEnabled}
                onCheckedChange={setSyncEnabled}
              />
              <Label htmlFor="cap-auto" className="text-sm cursor-pointer">
                {t('providers.capacity.autoAnalysis')}
              </Label>
            </div>

            <Button
              size="sm"
              onClick={handleSave}
              disabled={saveMutation.isPending}
              className="pb-0.5"
            >
              {saveMutation.isPending ? t('providers.capacity.saving') : t('common.save')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* VRAM pool view */}
      {capacityLoading && (
        <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
      )}

      {!capacityLoading && providers.length === 0 && (
        <Card className="border-dashed">
          <CardContent className="p-6 text-center text-sm text-muted-foreground">
            <Activity className="h-8 w-8 mx-auto mb-2 opacity-25" />
            {t('providers.capacity.noData')}
          </CardContent>
        </Card>
      )}

      {providers.map((provider) => (
        <Card key={provider.provider_id}>
          <CardContent className="p-0">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
              <Server className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
              <span className="text-sm font-semibold text-text-bright">{provider.provider_name}</span>
              <ThermalBadge state={provider.thermal_state} />
              {provider.temp_c !== null && (
                <span className="text-xs text-muted-foreground ml-1">{provider.temp_c.toFixed(1)}°C</span>
              )}
              <div className="ml-auto flex items-center gap-2">
                <span className="text-xs text-muted-foreground tabular-nums">
                  {fmtMbShort(provider.used_vram_mb)} / {fmtMbShort(provider.total_vram_mb)}
                </span>
                <VramBar used={provider.used_vram_mb} total={provider.total_vram_mb} />
              </div>
            </div>

            {provider.loaded_models.length === 0 ? (
              <p className="px-4 py-4 text-xs text-muted-foreground italic">{t('providers.capacity.noData')}</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-border bg-muted/30">
                      <th className="px-4 py-2 text-left font-medium text-muted-foreground">Model</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">Weight</th>
                      <th className="px-3 py-2 text-right font-medium text-muted-foreground">KV/req</th>
                      <th className="px-3 py-2 text-center font-medium text-muted-foreground">Active / Limit</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {provider.loaded_models.map((m) => (
                      <>
                        <tr key={m.model_name} className="hover:bg-muted/20 transition-colors">
                          <td className="px-4 py-2.5">
                            <span className="font-mono font-medium text-text-bright">{m.model_name}</span>
                          </td>
                          <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                            {fmtMbShort(m.weight_mb)}
                          </td>
                          <td className="px-3 py-2.5 text-right font-mono text-muted-foreground tabular-nums">
                            {fmtMbShort(m.kv_per_request_mb)}
                          </td>
                          <td className="px-3 py-2.5 text-center tabular-nums text-muted-foreground">
                            {m.active_requests}{m.max_concurrent > 0 ? `/${m.max_concurrent}` : ''}
                          </td>
                        </tr>
                        {m.llm_concern && (
                          <tr key={`${m.model_name}-concern`} className="bg-status-warn/5">
                            <td colSpan={4} className="px-4 py-1.5">
                              <span className="text-[10px] font-semibold text-status-warn-fg uppercase tracking-wide mr-2">
                                {t('providers.capacity.concern')}
                              </span>
                              <span className="text-xs text-muted-foreground">{m.llm_concern}</span>
                              {m.llm_reason && (
                                <span className="text-xs text-muted-foreground/70 ml-1">— {m.llm_reason}</span>
                              )}
                            </td>
                          </tr>
                        )}
                      </>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  )
}

// ── Gemini Status Sync Section ─────────────────────────────────────────────────

function statusDotCls(s: string) { return PROVIDER_STATUS_DOT_ALT[s] ?? PROVIDER_STATUS_DOT_ALT.offline }
function statusResultCls(s: string) { return PROVIDER_STATUS_TEXT[s] ?? PROVIDER_STATUS_TEXT.offline }
function statusResultLabel(s: string, t: (k: string) => string) {
  const key = PROVIDER_STATUS_I18N[s] ?? PROVIDER_STATUS_I18N.offline
  return t(key)
}

function GeminiStatusSyncSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiStatus(),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['providers'] })
    },
  })

  const results: GeminiStatusResult[] = syncMutation.data?.results ?? []
  const onlineCount = results.filter((r) => r.status === 'online').length

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <RefreshCw className="h-4 w-4 text-accent-gpu" />
        {t('providers.gemini.statusSyncSection')}
      </h2>

      <Card>
        <CardContent className="p-4 space-y-4">
          <p className="text-sm text-muted-foreground">{t('providers.gemini.statusSyncDesc')}</p>

          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="gap-1.5">
              <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('providers.gemini.syncingStatus') : t('providers.gemini.syncStatus')}
            </Button>
            {syncMutation.isSuccess && !syncMutation.isPending && (
              <span className="text-xs text-status-success-fg">
                ✓ {t('providers.gemini.statusSyncDone')} — {onlineCount}/{results.length} {t('common.online').toLowerCase()}
              </span>
            )}
          </div>

          {syncMutation.isSuccess && results.length === 0 && (
            <p className="text-xs text-muted-foreground italic">{t('providers.gemini.noStatusResults')}</p>
          )}

          {results.length > 0 && (
            <div className="divide-y divide-border rounded-md border border-border overflow-hidden">
              {results.map((r) => (
                <div key={r.id} className="flex items-center gap-3 px-3 py-2.5">
                  <span className={statusDotCls(r.status)} />
                  <span className="font-medium text-sm text-text-bright flex-1 truncate">{r.name}</span>
                  <span className={`text-xs font-medium ${statusResultCls(r.status)}`}>
                    {statusResultLabel(r.status, t)}
                  </span>
                  {r.error && (
                    <span className="text-xs text-status-error-fg truncate max-w-[160px]" title={r.error}>
                      {r.error}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// ── Gemini Sync Section ────────────────────────────────────────────────────────

function GeminiSyncSection() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const queryClient = useQueryClient()
  const [showSetKey, setShowSetKey] = useState(false)
  const [editingPolicy, setEditingPolicy] = useState<GeminiRateLimitPolicy | null>(null)

  // SSOT: all Gemini data refresh in one place — used by sync button and refresh button
  function refreshGeminiData() {
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.models })
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.policies })
    // Also refresh per-provider model selections so ModelSelectionModal picks up new models
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.selectedModels })
  }

  const { data: syncConfig } = useQuery(geminiSyncConfigQuery)

  const { data: modelsData, isLoading: modelsLoading, isFetching: modelsFetching } = useQuery(geminiModelsQuery)

  const { data: policies, isLoading: policiesLoading, isFetching: policiesFetching } = useQuery(geminiPoliciesQuery)

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiModels(),
    onSettled: () => refreshGeminiData(),
  })

  const isRefreshing = (modelsFetching || policiesFetching) && !syncMutation.isPending

  const models = modelsData?.models ?? []
  const lastSynced = models.length > 0
    ? fmtDatetimeShort(models[0].synced_at, tz)
    : null

  const policyMap = new Map<string, GeminiRateLimitPolicy>((policies ?? []).map(p => [p.model_name, p]))
  const globalDefault = policyMap.get('*')
  const syncedRows = [...models].sort((a, b) => a.model_name.localeCompare(b.model_name))

  function makeEditablePolicy(modelName: string): GeminiRateLimitPolicy {
    const existing = policyMap.get(modelName)
    if (existing) return existing
    return {
      id: '',
      model_name: modelName,
      rpm_limit: globalDefault?.rpm_limit ?? 0,
      rpd_limit: globalDefault?.rpd_limit ?? 0,
      available_on_free_tier: globalDefault?.available_on_free_tier ?? true,
      updated_at: '',
    }
  }

  const tableLoading = modelsLoading || policiesLoading
  const hasContent = !!globalDefault || models.length > 0

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
          <RotateCcw className="h-4 w-4 text-accent-gpu" />
          {t('providers.gemini.syncSection')}
        </h2>
        <p className="text-sm text-muted-foreground mt-0.5">{t('providers.gemini.syncSectionDesc')}</p>
      </div>

      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <p className="text-sm font-medium">{t('providers.gemini.syncKey')}</p>
              <p className="font-mono text-xs text-muted-foreground mt-0.5 truncate">
                {syncConfig?.api_key_masked ?? <span className="italic">{t('providers.gemini.noSyncKey')}</span>}
              </p>
            </div>
            <Button size="sm" variant="outline" onClick={() => setShowSetKey(true)} className="shrink-0">
              {syncConfig?.api_key_masked ? t('common.edit') : t('providers.gemini.setSyncKey')}
            </Button>
          </div>

          <div className="flex items-center gap-3 flex-wrap">
            <Button size="sm" onClick={() => syncMutation.mutate()}
              disabled={syncMutation.isPending || !syncConfig?.api_key_masked}
              className="gap-1.5">
              <RotateCcw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('common.syncing') : t('providers.gemini.syncNow')}
            </Button>
            <Button size="sm" variant="outline" onClick={refreshGeminiData}
              disabled={isRefreshing || syncMutation.isPending}
              className="gap-1.5">
              <RefreshCw className={isRefreshing ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {t('common.refresh')}
            </Button>
            {lastSynced && (
              <span className="text-xs text-muted-foreground">
                {t('providers.gemini.lastSynced')}: {lastSynced}
              </span>
            )}
            {syncMutation.data && (
              <span className="text-xs text-status-success-fg">
                ✓ {syncMutation.data.count} {t('providers.gemini.globalModels').toLowerCase()}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-accent-gpu" />
          <h3 className="text-sm font-semibold text-text-bright">{t('providers.gemini.rateLimitPolicies')}</h3>
        </div>
        <p className="text-sm text-muted-foreground">
          {t('providers.gemini.rateLimitDesc')}
          {' '}{t('providers.gemini.globalFallbackHint')}
        </p>

        {tableLoading && (
          <div className="flex h-16 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!tableLoading && !hasContent && (
          <Card className="border-dashed">
            <CardContent className="p-6 text-center text-muted-foreground text-sm">
              {t('providers.gemini.noGlobalModels')}
            </CardContent>
          </Card>
        )}

        {!tableLoading && hasContent && (
          <DataTable minWidth="600px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('providers.gemini.model')}</TableHead>
                <TableHead className="w-36">{t('providers.gemini.onFreeTier')}</TableHead>
                <TableHead className="w-24 text-right">{t('providers.gemini.rpm')}</TableHead>
                <TableHead className="w-24 text-right">{t('providers.gemini.rpd')}</TableHead>
                <TableHead className="w-40">{t('providers.gemini.lastUpdated')}</TableHead>
                <TableHead className="text-right w-20">{t('common.edit')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {syncedRows.map((m) => {
                const specific = policyMap.get(m.model_name)
                const isInherited = !specific
                const displayPolicy = specific ?? globalDefault
                return (
                  <TableRow key={m.model_name} className={isInherited ? 'opacity-60' : ''}>
                    <TableCell>
                      <span className="font-mono text-sm text-text-bright">{m.model_name}</span>
                    </TableCell>
                    <TableCell>
                      {isInherited ? (
                        <span className="text-xs text-muted-foreground italic">{t('providers.gemini.globalDefault')}</span>
                      ) : displayPolicy?.available_on_free_tier ? (
                        <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                          {t('providers.gemini.enabled')}
                        </Badge>
                      ) : (
                        <Badge variant="outline" className="bg-surface-code text-muted-foreground/70 border-border text-[10px] px-1.5 py-0">
                          {t('providers.gemini.paidOnly')}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums font-mono text-sm">
                      {displayPolicy && displayPolicy.rpm_limit > 0
                        ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpm_limit}</span>
                        : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-right tabular-nums font-mono text-sm">
                      {displayPolicy && displayPolicy.rpd_limit > 0
                        ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpd_limit}</span>
                        : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {specific?.updated_at ? fmtDateOnly(specific.updated_at, tz) : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button variant="ghost" size="icon"
                        className="h-8 w-8 text-muted-foreground hover:text-status-info-fg hover:bg-status-info/10"
                        onClick={() => setEditingPolicy(makeEditablePolicy(m.model_name))}
                        title={t('providers.gemini.editPolicyTitle')}>
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </DataTable>
        )}
      </div>

      {showSetKey && (
        <SetSyncKeyModal current={syncConfig?.api_key_masked ?? null} onClose={() => setShowSetKey(false)} />
      )}
      {editingPolicy && (
        <EditPolicyModal policy={editingPolicy} onClose={() => setEditingPolicy(null)} />
      )}
    </div>
  )
}

// ── Tab: Ollama providers ───────────────────────────────────────────────────────

function OllamaTab({
  providers,
  servers,
  isLoading,
  error,
  onRegister,
  onEdit,
  onSync,
  syncPending,
  syncVars,
  onDelete,
  deleteIsPending,
}: {
  providers: Provider[] | undefined
  servers: GpuServer[]
  isLoading: boolean
  error: Error | null
  onRegister: () => void
  onEdit: (b: Provider) => void
  onSync: (id: string) => void
  syncPending: boolean
  syncVars: string | undefined
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const ollama = providers?.filter((b) => b.provider_type === PROVIDER_OLLAMA) ?? []
  const serverMap = new Map(servers.map((s) => [s.id, s]))
  const ollamaCounts = ollama.reduce((acc, b) => {
    acc[b.status] = (acc[b.status] ?? 0) + 1
    return acc
  }, {} as Record<string, number>)
  const onlineCount = ollamaCounts['online'] ?? 0
  const offlineCount = ollamaCounts['offline'] ?? 0
  const degradedCount = ollamaCounts['degraded'] ?? 0
  const [viewModelsProvider, setViewModelsProvider] = useState<Provider | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)
  const [page, setPage] = useState(1)
  const totalPages = Math.max(1, Math.ceil(ollama.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages)
  const pageStart = (safePage - 1) * PAGE_SIZE
  const pageItems = ollama.slice(pageStart, pageStart + PAGE_SIZE)

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        {providers ? (
          <div className="flex items-center gap-2 flex-wrap">
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
              <Server className="h-3 w-3 shrink-0" />
              <span className="tabular-nums">{ollama.length}</span>
              <span>{t('providers.servers.registered')}</span>
            </div>
            {onlineCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-success/10 border border-status-success/30 text-xs font-medium text-status-success-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
                <span className="tabular-nums">{onlineCount}</span>
                <span>{t('common.online')}</span>
              </div>
            )}
            {degradedCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-warn/10 border border-status-warn/30 text-xs font-medium text-status-warn-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-warn shrink-0" />
                <span className="tabular-nums">{degradedCount}</span>
                <span>{t('common.degraded')}</span>
              </div>
            )}
            {offlineCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-error/10 border border-status-error/30 text-xs font-medium text-status-error-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
                <span className="tabular-nums">{offlineCount}</span>
                <span>{t('common.offline')}</span>
              </div>
            )}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
        )}

        <Button onClick={onRegister} className="shrink-0">
          <Plus className="h-4 w-4 mr-2" />{t('providers.ollama.registerProvider')}
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground text-sm animate-pulse">
          {t('providers.ollama.loadingBackends')}
        </div>
      )}

      {error && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardContent className="p-5 text-destructive">
            <p className="font-semibold">{t('providers.ollama.failedBackends')}</p>
            <p className="text-sm mt-1 opacity-75">
              {error instanceof Error ? error.message : t('common.unknownError')}
            </p>
          </CardContent>
        </Card>
      )}

      {!isLoading && ollama.length === 0 && !error && (
        <Card className="border-dashed">
          <CardContent className="p-10 text-center text-muted-foreground">
            <Server className="h-10 w-10 mx-auto mb-3 opacity-25" />
            <p className="font-medium">{t('providers.ollama.noBackends')}</p>
            <p className="text-sm mt-1">{t('providers.ollama.noBackendsHint')}</p>
          </CardContent>
        </Card>
      )}

      {ollama.length > 0 && (
        <DataTable
          minWidth="800px"
          footer={totalPages > 1 ? (
            <div className="flex items-center justify-between px-6 py-2">
              <span className="text-xs text-muted-foreground">
                {pageStart + 1}–{Math.min(pageStart + PAGE_SIZE, ollama.length)} / {ollama.length}
              </span>
              <div className="flex items-center gap-1">
                <Button variant="outline" size="icon" className="h-7 w-7"
                  onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={safePage <= 1}>
                  <ChevronLeft className="h-3.5 w-3.5" />
                </Button>
                <span className="text-xs text-muted-foreground px-1">{safePage} / {totalPages}</span>
                <Button variant="outline" size="icon" className="h-7 w-7"
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={safePage >= totalPages}>
                  <ChevronRight className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          ) : undefined}
        >
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>{t('providers.ollama.name')}</TableHead>
              <TableHead>{t('providers.ollama.server')}</TableHead>
              <TableHead className="min-w-52">{t('providers.servers.liveMetrics')}</TableHead>
              <TableHead className="w-28">{t('providers.ollama.status')}</TableHead>
              <TableHead className="w-32">{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="text-right w-44">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
              <TableBody>
                {pageItems.map((b) => {
                  const linkedServer = b.server_id ? serverMap.get(b.server_id) : null
                  return (
                    <TableRow key={b.id}>
                      <TableCell>
                        <div className="flex items-center gap-2 mb-1">
                          <span className="font-semibold text-text-bright">{b.name}</span>
                          {b.is_free_tier && (
                            <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                              {t('providers.ollama.freeTier')}
                            </Badge>
                          )}
                        </div>
                        {b.url && (
                          <span className="font-mono text-xs text-muted-foreground/70">{extractHost(b.url)}</span>
                        )}
                      </TableCell>

                      <TableCell>
                        <div className="space-y-1 text-xs">
                          {linkedServer ? (
                            <div className="flex items-center gap-1.5 text-text-dim">
                              <Server className="h-3 w-3 text-muted-foreground/70 shrink-0" />
                              <span className="font-medium">{linkedServer.name}</span>
                            </div>
                          ) : (
                            <span className="text-text-faint italic text-xs">{t('providers.ollama.noServerLinked')}</span>
                          )}
                          <div className="flex items-center gap-3 text-muted-foreground pl-0.5">
                            {b.gpu_index !== null && (
                              <span className="flex items-center gap-1">
                                <span className="text-[10px] font-semibold text-muted-foreground/70 uppercase">GPU</span>
                                <span className="tabular-nums font-mono">{b.gpu_index}</span>
                              </span>
                            )}
                            {b.total_vram_mb > 0 && (
                              <span className="flex items-center gap-1">
                                <span className="text-[10px] font-semibold text-muted-foreground/70 uppercase">VRAM</span>
                                <span className="tabular-nums font-mono">{fmtMb(b.total_vram_mb)}</span>
                              </span>
                            )}
                            {b.gpu_index === null && b.total_vram_mb === 0 && linkedServer && (
                              <span className="text-text-faint italic">{t('providers.servers.notConfigured')}</span>
                            )}
                          </div>
                        </div>
                      </TableCell>

                      <TableCell>
                        {linkedServer
                          ? <ServerMetricsCompact serverId={linkedServer.id} gpuIndex={b.gpu_index} />
                          : <span className="text-xs text-text-faint italic">—</span>
                        }
                      </TableCell>

                      <TableCell>
                        <StatusBadge status={b.status} />
                      </TableCell>

                      <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                        {fmtDateOnly(b.registered_at, tz)}
                      </TableCell>

                      <TableCell className="text-right">
                        <TooltipProvider delayDuration={200}>
                          <div className="flex items-center justify-end gap-1">
                            {linkedServer && (
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button variant="ghost" size="icon"
                                    className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                                    onClick={() => setHistoryServer(linkedServer)}>
                                    <BarChart2 className="h-4 w-4" />
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent>{t('providers.servers.history')}</TooltipContent>
                              </Tooltip>
                            )}
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button variant="ghost" size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-foreground"
                                  onClick={() => onSync(b.id)}
                                  disabled={syncPending && syncVars === b.id}>
                                  <RefreshCw className={
                                    syncPending && syncVars === b.id
                                      ? 'h-4 w-4 animate-spin' : 'h-4 w-4'
                                  } />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>Sync</TooltipContent>
                            </Tooltip>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button variant="ghost" size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                                  onClick={() => setViewModelsProvider(b)}>
                                  <ListFilter className="h-4 w-4" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>{t('providers.ollama.modelSelection')}</TooltipContent>
                            </Tooltip>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button variant="ghost" size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10"
                                  onClick={() => onEdit(b)}>
                                  <Pencil className="h-4 w-4" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>{t('providers.ollama.editTitle')}</TooltipContent>
                            </Tooltip>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button variant="ghost" size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                                  onClick={() => onDelete(b.id, b.name)}
                                  disabled={deleteIsPending}>
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>{t('providers.removeProvider')}</TooltipContent>
                            </Tooltip>
                          </div>
                        </TooltipProvider>
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
        </DataTable>
      )}

      <OllamaSyncSection />

      <OllamaCapacitySection />

      {viewModelsProvider && (
        <OllamaProviderModelsModal
          provider={viewModelsProvider}
          onClose={() => setViewModelsProvider(null)}
        />
      )}
      {historyServer && (
        <ServerHistoryModal server={historyServer} onClose={() => setHistoryServer(null)} />
      )}
    </div>
  )
}

// ── Tab: Gemini providers + policies ───────────────────────────────────────────

function GeminiTab({
  providers,
  isLoading,
  error,
  onRegister,
  onEdit,
  onSync,
  syncPending,
  onToggleActive,
  toggleActivePending,
  onDelete,
  deleteIsPending,
}: {
  providers: Provider[] | undefined
  isLoading: boolean
  error: Error | null
  onRegister: () => void
  onEdit: (b: Provider) => void
  onSync: (id: string) => void
  syncPending: boolean
  onToggleActive: (b: Provider) => void
  toggleActivePending: boolean
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const gemini = providers?.filter((b) => b.provider_type === PROVIDER_GEMINI) ?? []
  const geminiCounts = gemini.reduce((acc, b) => {
    acc[b.status] = (acc[b.status] ?? 0) + 1
    if (b.is_active) acc['_active'] = (acc['_active'] ?? 0) + 1
    return acc
  }, {} as Record<string, number>)
  const onlineCount = geminiCounts['online'] ?? 0
  const activeCount = geminiCounts['_active'] ?? 0
  const degradedCount = geminiCounts['degraded'] ?? 0
  const offlineCount = geminiCounts['offline'] ?? 0
  const [modelSelectionProvider, setModelSelectionProvider] = useState<Provider | null>(null)
  const [geminiPage, setGeminiPage] = useState(1)
  const geminiTotalPages = Math.max(1, Math.ceil(gemini.length / PAGE_SIZE))
  const geminiSafePage = Math.min(geminiPage, geminiTotalPages)
  const geminiPageStart = (geminiSafePage - 1) * PAGE_SIZE
  const geminiPageItems = gemini.slice(geminiPageStart, geminiPageStart + PAGE_SIZE)

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-text-bright">{t('providers.gemini.title')}</h2>
            {providers ? (
              <div className="flex items-center gap-2 flex-wrap mt-1.5">
                <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
                  <Key className="h-3 w-3 shrink-0" />
                  <span className="tabular-nums">{gemini.length}</span>
                  <span>{t('providers.servers.registered')}</span>
                </div>
                {activeCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 border border-primary/30 text-xs font-medium text-primary">
                    <ShieldCheck className="h-3 w-3 shrink-0" />
                    <span className="tabular-nums">{activeCount}</span>
                    <span>{t('common.active')}</span>
                  </div>
                )}
                {onlineCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-success/10 border border-status-success/30 text-xs font-medium text-status-success-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
                    <span className="tabular-nums">{onlineCount}</span>
                    <span>{t('common.online')}</span>
                  </div>
                )}
                {degradedCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-warn/10 border border-status-warn/30 text-xs font-medium text-status-warn-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-warn shrink-0" />
                    <span className="tabular-nums">{degradedCount}</span>
                    <span>{t('common.degraded')}</span>
                  </div>
                )}
                {offlineCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-error/10 border border-status-error/30 text-xs font-medium text-status-error-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
                    <span className="tabular-nums">{offlineCount}</span>
                    <span>{t('common.offline')}</span>
                  </div>
                )}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground mt-0.5 animate-pulse">{t('common.loading')}</p>
            )}
          </div>
          <Button onClick={onRegister} className="shrink-0">
            <Plus className="h-4 w-4 mr-2" />{t('providers.gemini.registerProvider')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('providers.gemini.loadingBackends')}
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">{t('providers.gemini.failedBackends')}</p>
              <p className="text-sm mt-1 opacity-75">
                {error instanceof Error ? error.message : t('common.unknownError')}
              </p>
            </CardContent>
          </Card>
        )}

        {!isLoading && gemini.length === 0 && !error && (
          <Card className="border-dashed">
            <CardContent className="p-10 text-center text-muted-foreground">
              <Key className="h-10 w-10 mx-auto mb-3 opacity-25" />
              <p className="font-medium text-text-dim">{t('providers.gemini.noBackends')}</p>
              <p className="text-sm mt-1 text-muted-foreground/70">{t('providers.gemini.noBackendsHint')}</p>
            </CardContent>
          </Card>
        )}

        {gemini.length > 0 && (
          <DataTable
            minWidth="760px"
            footer={geminiTotalPages > 1 ? (
              <div className="flex items-center justify-between px-6 py-2">
                <span className="text-xs text-muted-foreground">
                  {geminiPageStart + 1}–{Math.min(geminiPageStart + PAGE_SIZE, gemini.length)} / {gemini.length}
                </span>
                <div className="flex items-center gap-1">
                  <Button variant="outline" size="icon" className="h-7 w-7"
                    onClick={() => setGeminiPage((p) => Math.max(1, p - 1))} disabled={geminiSafePage <= 1}>
                    <ChevronLeft className="h-3.5 w-3.5" />
                  </Button>
                  <span className="text-xs text-muted-foreground px-1">{geminiSafePage} / {geminiTotalPages}</span>
                  <Button variant="outline" size="icon" className="h-7 w-7"
                    onClick={() => setGeminiPage((p) => Math.min(geminiTotalPages, p + 1))} disabled={geminiSafePage >= geminiTotalPages}>
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ) : undefined}
          >
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('providers.gemini.name')}</TableHead>
                <TableHead>{t('providers.gemini.apiKey')}</TableHead>
                <TableHead className="w-24">{t('providers.gemini.freeTier')}</TableHead>
                <TableHead className="w-24">{t('providers.gemini.activeToggle')}</TableHead>
                <TableHead className="w-28">{t('providers.gemini.status')}</TableHead>
                <TableHead className="w-32">{t('providers.servers.registeredAt')}</TableHead>
                <TableHead className="text-right w-28">{t('keys.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {geminiPageItems.map((b) => (
                <TableRow key={b.id} className={!b.is_active ? 'opacity-50' : ''}>
                  <TableCell>
                    <div className="font-semibold text-text-bright">{b.name}</div>
                  </TableCell>
                  <TableCell>
                    <ApiKeyCell providerId={b.id} masked={b.api_key_masked} />
                  </TableCell>
                  <TableCell>
                    {b.is_free_tier ? (
                      <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                        {t('providers.gemini.freeTier')}
                      </Badge>
                    ) : (
                      <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 text-[10px] px-1.5 py-0">
                        {t('providers.gemini.paid')}
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={b.is_active}
                      onCheckedChange={() => onToggleActive(b)}
                      disabled={toggleActivePending}
                      title={b.is_active ? t('providers.disableProvider') : t('providers.enableProvider')}
                    />
                  </TableCell>
                  <TableCell>
                    <StatusBadge status={b.status} />
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                    {fmtDateOnly(b.registered_at, tz)}
                  </TableCell>
                  <TableCell className="text-right">
                    <TooltipProvider delayDuration={200}>
                      <div className="flex items-center justify-end gap-1">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-muted-foreground hover:text-text-bright"
                              onClick={() => onSync(b.id)}
                              disabled={syncPending}>
                              <RefreshCw className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>Sync</TooltipContent>
                        </Tooltip>
                        {!b.is_free_tier && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <Button variant="ghost" size="icon"
                                className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                                onClick={() => setModelSelectionProvider(b)}>
                                <ListFilter className="h-4 w-4" />
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>{t('providers.gemini.modelSelection')}</TooltipContent>
                          </Tooltip>
                        )}
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10"
                              onClick={() => onEdit(b)}>
                              <Pencil className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.gemini.editTitle')}</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                              onClick={() => onDelete(b.id, b.name)}
                              disabled={deleteIsPending}>
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.removeProvider')}</TooltipContent>
                        </Tooltip>
                      </div>
                    </TooltipProvider>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </DataTable>
        )}
      </div>

      <GeminiStatusSyncSection />

      <GeminiSyncSection />

      {modelSelectionProvider && (
        <ModelSelectionModal
          provider={modelSelectionProvider}
          onClose={() => setModelSelectionProvider(null)}
        />
      )}
    </div>
  )
}

// ── Page ──────────────────────────────────────────────────────────────────────

function ProvidersContent({ section: sectionParam }: { section: string }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false
  // Fall back to 'ollama' when Gemini is disabled and the URL says ?s=gemini
  const section = (sectionParam === 'gemini' && !geminiEnabled) ? 'ollama' : sectionParam

  const [registerProviderType, setRegisterProviderType] = useState<'ollama' | 'gemini' | null>(null)
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null)

  // Servers needed for RegisterModal/EditModal dropdowns
  const { data: servers } = useQuery(serversQuery)

  const { data: providers, isLoading: providersLoading, error: providersError } = useQuery(providersQuery)

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteProvider(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  })

  const toggleActiveMutation = useMutation({
    mutationFn: (b: Provider) =>
      api.updateProvider(b.id, {
        name: b.name,
        is_active: !b.is_active,
        ...(b.provider_type === PROVIDER_OLLAMA && { url: b.url, total_vram_mb: b.total_vram_mb, gpu_index: b.gpu_index, server_id: b.server_id }),
      }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  })

  const syncProviderMutation = useMutation({
    mutationFn: (id: string) => api.syncProvider(id),
    onSettled: (_data, _error, id) => {
      queryClient.invalidateQueries({ queryKey: ['providers'] })
      queryClient.invalidateQueries({ queryKey: ['provider-models', id] })
      queryClient.invalidateQueries({ queryKey: ['selected-models', id] })
      queryClient.invalidateQueries({ queryKey: ['ollama-sync-status'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
      queryClient.invalidateQueries({ queryKey: ['capacity'] })
    },
  })

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('providers.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('providers.description')}</p>
      </div>

      {section === 'ollama' && (
        <OllamaTab
          providers={providers}
          servers={servers ?? []}
          isLoading={providersLoading}
          error={providersError as Error | null}
          onRegister={() => setRegisterProviderType('ollama')}
          onEdit={(b) => setEditingProvider(b)}
          onSync={(id) => syncProviderMutation.mutate(id)}
          syncPending={syncProviderMutation.isPending}
          syncVars={syncProviderMutation.variables}
          onDelete={(id, name) => { if (confirm(t('providers.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {section === 'gemini' && (
        <GeminiTab
          providers={providers}
          isLoading={providersLoading}
          error={providersError as Error | null}
          onRegister={() => setRegisterProviderType('gemini')}
          onEdit={(b) => setEditingProvider(b)}
          onSync={(id) => syncProviderMutation.mutate(id)}
          syncPending={syncProviderMutation.isPending}
          onToggleActive={(b) => toggleActiveMutation.mutate(b)}
          toggleActivePending={toggleActiveMutation.isPending}
          onDelete={(id, name) => { if (confirm(t('providers.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {registerProviderType && (
        <RegisterModal
          servers={servers ?? []}
          initialType={registerProviderType}
          onClose={() => setRegisterProviderType(null)}
        />
      )}
      {editingProvider && (
        <EditModal
          provider={editingProvider}
          servers={servers ?? []}
          onClose={() => setEditingProvider(null)}
        />
      )}
    </div>
  )
}

function ProvidersSectionReader() {
  const searchParams = useSearchParams()
  const section = searchParams.get('s') ?? 'ollama'
  return <ProvidersContent section={section} />
}

export default function ProvidersPage() {
  return (
    <Suspense fallback={<div className="p-2 text-sm text-muted-foreground">Loading…</div>}>
      <ProvidersSectionReader />
    </Suspense>
  )
}
