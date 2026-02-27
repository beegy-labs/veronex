'use client'

import { useState, Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Backend, BackendSelectedModel, GeminiModel, GeminiRateLimitPolicy, GeminiStatusResult, GeminiStatusSyncResponse, GeminiSyncConfig, GpuServer, NodeMetrics, OllamaBackendForModel, OllamaModelWithCount, OllamaSyncJob, RegisterBackendRequest, UpdateBackendRequest } from '@/lib/types'
import { Plus, Trash2, RefreshCw, RotateCcw, Server, Key, Wifi, WifiOff, AlertCircle, Thermometer, Zap, Pencil, MemoryStick, ShieldCheck, Eye, EyeOff, ListFilter, Search, Cpu, ChevronLeft, ChevronRight } from 'lucide-react'
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
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'

// ── Helpers ────────────────────────────────────────────────────────────────────

function extractHost(url: string): string {
  try { return new URL(url).host } catch { return url }
}

function fmtMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

// ── Status badge ───────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: Backend['status'] }) {
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

// ── Compact inline metrics for Ollama server cells ─────────────────────────────

function OllamaServerMetrics({ serverId, gpuIndex }: { serverId: string; gpuIndex: number | null }) {
  const { t } = useTranslation()
  const { data, isError } = useQuery<NodeMetrics>({
    queryKey: ['server-metrics', serverId],
    queryFn: () => api.serverMetrics(serverId),
    refetchInterval: 30_000,
    retry: false,
  })

  if (isError || (data && !data.scrape_ok)) {
    return <span className="text-[10px] text-status-error-fg italic">{t('backends.servers.unreachable')}</span>
  }
  if (!data) return null

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const gpu = data.gpus[gpuIndex ?? 0] ?? null
  const tempCls = gpu?.temp_c != null && gpu.temp_c >= 85
    ? 'text-status-error-fg'
    : gpu?.temp_c != null && gpu.temp_c >= 70
    ? 'text-status-warn-fg'
    : 'text-muted-foreground'

  return (
    <div className="mt-1.5 pt-1.5 border-t border-border/40 flex flex-wrap items-center gap-x-2.5 gap-y-0.5">
      <span className="flex items-center gap-1">
        <span className="text-[10px] font-semibold text-muted-foreground/60 uppercase">MEM</span>
        <span className="tabular-nums font-mono text-[11px] text-text-dim">
          {fmtMb(memUsed)}<span className="text-muted-foreground/40">/{fmtMb(data.mem_total_mb)}</span>
        </span>
      </span>
      {gpu?.temp_c != null && (
        <span className={`flex items-center gap-0.5 text-[11px] tabular-nums ${tempCls}`}>
          <Thermometer className="h-3 w-3 shrink-0" />{gpu.temp_c.toFixed(0)}°C
        </span>
      )}
      {gpu?.power_w != null && (
        <span className="flex items-center gap-0.5 text-[11px] tabular-nums text-muted-foreground">
          <Zap className="h-3 w-3 shrink-0 text-accent-power" />{gpu.power_w.toFixed(0)}W
        </span>
      )}
    </div>
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

// ── Edit backend modal ─────────────────────────────────────────────────────────

function EditModal({ backend, servers, onClose }: { backend: Backend; servers: GpuServer[]; onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(backend.name)
  const [url, setUrl] = useState(backend.url)
  const [apiKey, setApiKey] = useState('')
  const [vram, setVram] = useState(backend.total_vram_mb > 0 ? String(backend.total_vram_mb) : '')
  const [gpuIndex, setGpuIndex] = useState(backend.gpu_index !== null ? String(backend.gpu_index) : 'none')
  const [serverId, setServerId] = useState<string>(backend.server_id ?? 'none')
  const [isFreeTier, setIsFreeTier] = useState(backend.is_free_tier)

  const { data: serverMetrics } = useQuery<NodeMetrics>({
    queryKey: ['server-metrics', serverId],
    queryFn: () => api.serverMetrics(serverId),
    enabled: serverId !== 'none',
    staleTime: 30_000,
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: UpdateBackendRequest = {
        name: name.trim(),
        url: backend.backend_type === 'ollama' ? url.trim() : undefined,
        api_key: apiKey.trim() || undefined,
        total_vram_mb: vram ? parseInt(vram, 10) : 0,
        gpu_index: gpuIndex !== 'none' && gpuIndex !== '' ? parseInt(gpuIndex, 10) : null,
        server_id: serverId !== 'none' ? serverId : null,
        ...(backend.backend_type === 'gemini' && { is_free_tier: isFreeTier }),
      }
      return api.updateBackend(backend.id, body)
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['backends'] }); onClose() },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('backends.editBackendTitle')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-name">{t('backends.ollama.name')} <span className="text-destructive">*</span></Label>
            <Input id="edit-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          {backend.backend_type === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="edit-url">{t('backends.ollama.ollamaUrl')}</Label>
                <Input id="edit-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="edit-server">
                  {t('backends.ollama.gpuServer')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="edit-server"><SelectValue placeholder={t('backends.ollama.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('backends.ollama.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-1.5">
                <Label>{t('backends.ollama.gpuIndex')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder={t('backends.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('backends.ollama.noneOption')}</SelectItem>
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
                <Label>{t('backends.ollama.maxVram')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span></Label>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>
            </>
          )}

          {backend.backend_type === 'gemini' && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="edit-apikey">
                  {t('backends.gemini.apiKey')} <span className="text-muted-foreground font-normal">— {t('backends.gemini.keepExistingKey')}</span>
                </Label>
                <Input id="edit-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
                <p className="text-xs text-muted-foreground">{t('backends.gemini.apiKeyHint')}</p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{t('backends.gemini.freeTier')}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('backends.gemini.freeTierDesc')}</p>
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

// ── Register backend modal ─────────────────────────────────────────────────────

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

  const { data: serverMetrics } = useQuery<NodeMetrics>({
    queryKey: ['server-metrics', serverId],
    queryFn: () => api.serverMetrics(serverId),
    enabled: serverId !== 'none',
    staleTime: 30_000,
  })
  const gpuCards = serverMetrics?.gpus ?? []
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: RegisterBackendRequest = {
        name: name.trim(),
        backend_type: initialType,
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
      return api.registerBackend(body)
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['backends'] }); onClose() },
  })

  const isValid = name.trim() && (initialType === 'ollama' ? url.trim() : apiKey.trim())

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {initialType === 'ollama'
              ? <><Server className="h-4 w-4 text-status-info-fg" /> {t('backends.ollama.registerTitle')}</>
              : <><Key className="h-4 w-4 text-accent-gpu" /> {t('backends.gemini.registerTitle')}</>}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="backend-name">{t('backends.ollama.name')} <span className="text-destructive">*</span></Label>
            <Input id="backend-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={initialType === 'ollama' ? 'e.g. gpu-server-1' : 'e.g. gemini-prod'} />
          </div>

          {initialType === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="backend-url">{t('backends.ollama.ollamaUrl')} <span className="text-destructive">*</span></Label>
                <Input id="backend-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://192.168.1.10:11434" />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="backend-server">
                  {t('backends.ollama.gpuServer')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="backend-server"><SelectValue placeholder={t('backends.ollama.noneOption')} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t('backends.ollama.noneOption')}</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">{t('backends.ollama.gpuServerHint')}</p>
              </div>

              <div className="space-y-1.5">
                <Label>{t('backends.ollama.gpuIndex')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder={t('backends.ollama.noneOption')} /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t('backends.ollama.noneOption')}</SelectItem>
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
                <Label>{t('backends.ollama.maxVram')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span></Label>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>
            </>
          )}

          {initialType === 'gemini' && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="backend-apikey">{t('backends.gemini.apiKey')} <span className="text-destructive">*</span></Label>
                <Input id="backend-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
                <p className="text-xs text-muted-foreground">{t('backends.gemini.apiKeyHint')}</p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{t('backends.gemini.freeTier')}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('backends.gemini.freeTierDesc')}</p>
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
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['gemini-policies'] }); onClose() },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-accent-gpu" />
            {t('backends.gemini.editPolicyTitle')}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-1 mb-1">
          <p className="text-sm text-muted-foreground">{t('backends.gemini.model')}</p>
          <p className="font-mono text-sm font-semibold text-text-bright">
            {policy.model_name === '*' ? `* (${t('backends.gemini.globalDefault')})` : policy.model_name}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
            <div>
              <p className="text-sm font-medium">{t('backends.gemini.availableOnFreeTier')}</p>
              <p className="text-xs text-muted-foreground mt-0.5">
                {availableOnFreeTier
                  ? t('backends.gemini.freeTierRouting')
                  : t('backends.gemini.paidOnlyRouting')}
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
                {t('backends.gemini.freeLimitsHint')}
              </p>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('backends.gemini.failedToSave')}
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

function ApiKeyCell({ backendId, masked }: { backendId: string; masked: string | null }) {
  const { t } = useTranslation()
  const [revealed, setRevealed] = useState(false)

  const { data, isFetching, refetch } = useQuery({
    queryKey: ['backend-key', backendId],
    queryFn: () => api.backendKey(backendId),
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

function ModelSelectionModal({ backend, onClose }: { backend: Backend; onClose: () => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const { data, isLoading } = useQuery<{ models: BackendSelectedModel[] }>({
    queryKey: ['selected-models', backend.id],
    queryFn: () => api.getSelectedModels(backend.id),
    staleTime: 0,
  })

  const toggleMutation = useMutation({
    mutationFn: ({ modelName, isEnabled }: { modelName: string; isEnabled: boolean }) =>
      api.setModelEnabled(backend.id, modelName, isEnabled),
    onMutate: async ({ modelName, isEnabled }) => {
      await queryClient.cancelQueries({ queryKey: ['selected-models', backend.id] })
      const prev = queryClient.getQueryData<{ models: BackendSelectedModel[] }>(['selected-models', backend.id])
      queryClient.setQueryData<{ models: BackendSelectedModel[] }>(['selected-models', backend.id], (old) => {
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
        queryClient.setQueryData(['selected-models', backend.id], context.prev)
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['selected-models', backend.id] })
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
            {t('backends.gemini.modelSelection')}
            <span className="text-muted-foreground font-normal text-sm">— {backend.name}</span>
          </DialogTitle>
        </DialogHeader>

        <p className="text-xs text-muted-foreground -mt-1">
          {t('backends.gemini.modelSelectionDesc')}
        </p>

        {isLoading && (
          <div className="flex h-20 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && models.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center">
            {t('backends.gemini.noGlobalModels')}
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
            {t('backends.gemini.modelsCount', { enabled: enabledCount, total: models.length })}
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['gemini-sync-config'] })
      onClose()
    },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Key className="h-4 w-4 text-accent-gpu" />
            {t('backends.gemini.setSyncKey')}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          {current && (
            <p className="text-xs text-muted-foreground">
              {t('backends.gemini.syncKey')}: <span className="font-mono text-text-dim">{current}</span>
            </p>
          )}
          <div className="space-y-1.5">
            <Label htmlFor="sync-key">{t('backends.gemini.syncKey')} <span className="text-destructive">*</span></Label>
            <Input id="sync-key" type="password" value={apiKey}
              onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
            <p className="text-xs text-muted-foreground">{t('backends.gemini.syncKeyHint')}</p>
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

// ── OllamaModelBackendsModal ───────────────────────────────────────────────────

const BACKENDS_PAGE_SIZE = 8

function OllamaModelBackendsModal({ modelName, onClose }: { modelName: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)

  const { data, isLoading } = useQuery<{ backends: OllamaBackendForModel[] }>({
    queryKey: ['ollama-model-backends', modelName],
    queryFn: () => api.ollamaModelBackends(modelName),
    staleTime: 30_000,
  })

  const allBackends = data?.backends ?? []
  const filtered = allBackends.filter((b) =>
    b.name.toLowerCase().includes(search.toLowerCase()) ||
    b.url.toLowerCase().includes(search.toLowerCase())
  )

  const totalPages = Math.max(1, Math.ceil(filtered.length / BACKENDS_PAGE_SIZE))
  const safePage = Math.min(page, totalPages)
  const pageStart = (safePage - 1) * BACKENDS_PAGE_SIZE
  const pageItems = filtered.slice(pageStart, pageStart + BACKENDS_PAGE_SIZE)

  const handleSearch = (v: string) => { setSearch(v); setPage(1) }

  function statusDot(s: string) {
    if (s === 'online')   return 'h-2 w-2 rounded-full bg-status-success shrink-0'
    if (s === 'degraded') return 'h-2 w-2 rounded-full bg-status-warn shrink-0'
    return 'h-2 w-2 rounded-full bg-status-error shrink-0'
  }
  function statusBadgeCls(s: string) {
    if (s === 'online')   return 'text-status-success-fg border-status-success/40 text-[10px]'
    if (s === 'degraded') return 'text-status-warn-fg border-status-warn/40 text-[10px]'
    return 'text-status-error-fg border-status-error/40 text-[10px]'
  }
  function statusLabel(s: string) {
    if (s === 'online')   return t('common.online')
    if (s === 'degraded') return t('common.degraded')
    return t('common.offline')
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
            placeholder={t('backends.ollama.searchServers')}
            value={search}
            onChange={(e) => handleSearch(e.target.value)}
          />
        </div>

        {!isLoading && allBackends.length > 0 && (
          <p className="text-xs text-muted-foreground -mt-1">
            {filtered.length} / {allBackends.length} {t('backends.ollama.serversWithModel')}
            {search ? ` — "${search}"` : ''}
          </p>
        )}

        {isLoading && (
          <p className="text-sm text-muted-foreground py-4 text-center animate-pulse">{t('common.loading')}</p>
        )}

        {!isLoading && allBackends.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center italic">
            {t('backends.ollama.noBackendsSynced')}
          </p>
        )}

        {!isLoading && filtered.length === 0 && search && (
          <p className="text-sm text-muted-foreground py-3 text-center italic">
            {t('backends.ollama.noServersMatch')} &ldquo;{search}&rdquo;
          </p>
        )}

        {!isLoading && pageItems.length > 0 && (
          <div className="space-y-2">
            {pageItems.map((b) => (
              <div key={b.backend_id} className="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5">
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
              {pageStart + 1}–{Math.min(pageStart + BACKENDS_PAGE_SIZE, filtered.length)} / {filtered.length}
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

// ── OllamaBackendModelsModal ───────────────────────────────────────────────────

function OllamaBackendModelsModal({ backend, onClose }: { backend: Backend; onClose: () => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')

  const { data, isLoading } = useQuery<{ models: string[] }>({
    queryKey: ['ollama-backend-models', backend.id],
    queryFn: () => api.ollamaBackendModels(backend.id),
    staleTime: 30_000,
  })

  const models = (data?.models ?? []).filter((m) =>
    m.toLowerCase().includes(search.toLowerCase())
  )

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Cpu className="h-4 w-4 text-accent-gpu" />
            {backend.name}
          </DialogTitle>
        </DialogHeader>

        <div className="relative">
          <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground/60 pointer-events-none" />
          <Input
            className="pl-8 h-8 text-sm"
            placeholder={t('backends.ollama.ollamaSearchModels')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {isLoading && (
          <p className="text-sm text-muted-foreground py-4 text-center animate-pulse">{t('common.loading')}</p>
        )}

        {!isLoading && data?.models.length === 0 && (
          <p className="text-sm text-muted-foreground py-4 text-center italic">
            {t('backends.ollama.noBackendModels')}
          </p>
        )}

        {!isLoading && models.length === 0 && search && (
          <p className="text-sm text-muted-foreground py-2 text-center italic">
            {t('backends.ollama.noModelsMatch')} &ldquo;{search}&rdquo;
          </p>
        )}

        {!isLoading && models.length > 0 && (
          <div className="flex flex-wrap gap-1.5 max-h-64 overflow-y-auto py-1">
            {models.map((m) => (
              <Badge key={m} variant="outline" className="font-mono text-[11px] px-2 py-0.5">
                {m}
              </Badge>
            ))}
          </div>
        )}

        <DialogFooter>
          <span className="text-xs text-muted-foreground mr-auto">
            {t('backends.ollama.modelsCount', { count: data?.models.length ?? 0 })}
          </span>
          <Button variant="outline" size="sm" onClick={onClose}>{t('common.close')}</Button>
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

  const { data: syncJob } = useQuery<OllamaSyncJob>({
    queryKey: ['ollama-sync-status'],
    queryFn: () => api.ollamaSyncStatus(),
    refetchInterval: (query) => {
      const data = query.state.data as OllamaSyncJob | undefined
      return data?.status === 'running' ? 2000 : false
    },
    retry: false,
  })

  const { data: ollamaModelsData } = useQuery<{ models: OllamaModelWithCount[] }>({
    queryKey: ['ollama-models'],
    queryFn: () => api.ollamaModels(),
    staleTime: 30_000,
  })

  const syncMutation = useMutation({
    mutationFn: () => api.syncOllamaModels(),
    onSuccess: () => {
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
        {t('backends.ollama.ollamaSyncSection')}
      </h2>

      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={isRunning} className="gap-1.5">
              <RotateCcw className={isRunning ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {isRunning ? t('backends.ollama.ollamaSyncing') : t('backends.ollama.ollamaSyncAll')}
            </Button>
            {syncJob?.status === 'running' && (
              <span className="text-xs text-muted-foreground">
                {syncJob.done_backends}/{syncJob.total_backends}
              </span>
            )}
            {syncJob?.status === 'completed' && !syncMutation.isPending && (
              <span className="text-xs text-status-success-fg">✓ {t('backends.ollama.ollamaSyncDone')}</span>
            )}
          </div>

          {allModels.length === 0 && (
            <p className="text-xs text-muted-foreground italic">{t('backends.ollama.ollamaNoSync')}</p>
          )}

          {allModels.length > 0 && (
            <div className="space-y-3">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground/60 pointer-events-none" />
                <Input
                  className="pl-8 h-8 text-sm"
                  placeholder={t('backends.ollama.ollamaSearchModels')}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </div>
              <div className="flex items-center justify-between">
                <p className="text-xs font-medium text-muted-foreground">
                  {t('backends.ollama.ollamaAvailableModels')}
                </p>
                <span className="text-xs text-muted-foreground">{filteredModels.length}/{allModels.length}</span>
              </div>
              <div className="divide-y divide-border rounded-md border border-border overflow-hidden">
                {filteredModels.length === 0 && (
                  <p className="text-xs text-muted-foreground italic py-3 px-3">
                    {t('backends.ollama.noModelsMatch')} &ldquo;{search}&rdquo;
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
                      {m.backend_count}
                    </Badge>
                  </button>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {selectedModel && (
        <OllamaModelBackendsModal modelName={selectedModel} onClose={() => setSelectedModel(null)} />
      )}
    </div>
  )
}

// ── Gemini Status Sync Section ─────────────────────────────────────────────────

function statusDotCls(s: string) {
  if (s === 'online')   return 'h-2 w-2 rounded-full bg-status-success shrink-0'
  if (s === 'degraded') return 'h-2 w-2 rounded-full bg-status-warn shrink-0'
  return 'h-2 w-2 rounded-full bg-muted-foreground/40 shrink-0'
}
function statusResultCls(s: string) {
  if (s === 'online')   return 'text-status-success-fg'
  if (s === 'degraded') return 'text-status-warn-fg'
  return 'text-muted-foreground'
}
function statusResultLabel(s: string, t: (k: string) => string) {
  if (s === 'online')   return t('common.online')
  if (s === 'degraded') return t('common.degraded')
  return t('common.offline')
}

function GeminiStatusSyncSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiStatus(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backends'] })
    },
  })

  const results: GeminiStatusResult[] = syncMutation.data?.results ?? []
  const onlineCount = results.filter((r) => r.status === 'online').length

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <RefreshCw className="h-4 w-4 text-accent-gpu" />
        {t('backends.gemini.statusSyncSection')}
      </h2>

      <Card>
        <CardContent className="p-4 space-y-4">
          <p className="text-sm text-muted-foreground">{t('backends.gemini.statusSyncDesc')}</p>

          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="gap-1.5">
              <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('backends.gemini.syncingStatus') : t('backends.gemini.syncStatus')}
            </Button>
            {syncMutation.isSuccess && !syncMutation.isPending && (
              <span className="text-xs text-status-success-fg">
                ✓ {t('backends.gemini.statusSyncDone')} — {onlineCount}/{results.length} {t('common.online').toLowerCase()}
              </span>
            )}
          </div>

          {syncMutation.isSuccess && results.length === 0 && (
            <p className="text-xs text-muted-foreground italic">{t('backends.gemini.noStatusResults')}</p>
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
  const queryClient = useQueryClient()
  const [showSetKey, setShowSetKey] = useState(false)
  const [editingPolicy, setEditingPolicy] = useState<GeminiRateLimitPolicy | null>(null)

  const { data: syncConfig } = useQuery<GeminiSyncConfig>({
    queryKey: ['gemini-sync-config'],
    queryFn: () => api.geminiSyncConfig(),
    staleTime: 30_000,
  })

  const { data: modelsData, isLoading: modelsLoading } = useQuery<{ models: GeminiModel[] }>({
    queryKey: ['gemini-models'],
    queryFn: () => api.geminiModels(),
    staleTime: 30_000,
  })

  const { data: policies, isLoading: policiesLoading } = useQuery({
    queryKey: ['gemini-policies'],
    queryFn: () => api.geminiPolicies(),
    staleTime: 30_000,
  })

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiModels(),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['gemini-models'] }) },
  })

  const models = modelsData?.models ?? []
  const lastSynced = models.length > 0
    ? new Date(models[0].synced_at).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
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
          {t('backends.gemini.syncSection')}
        </h2>
        <p className="text-sm text-muted-foreground mt-0.5">{t('backends.gemini.syncSectionDesc')}</p>
      </div>

      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <p className="text-sm font-medium">{t('backends.gemini.syncKey')}</p>
              <p className="font-mono text-xs text-muted-foreground mt-0.5 truncate">
                {syncConfig?.api_key_masked ?? <span className="italic">{t('backends.gemini.noSyncKey')}</span>}
              </p>
            </div>
            <Button size="sm" variant="outline" onClick={() => setShowSetKey(true)} className="shrink-0">
              {syncConfig?.api_key_masked ? t('common.edit') : t('backends.gemini.setSyncKey')}
            </Button>
          </div>

          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()}
              disabled={syncMutation.isPending || !syncConfig?.api_key_masked}
              className="gap-1.5">
              <RotateCcw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('common.syncing') : t('backends.gemini.syncNow')}
            </Button>
            {lastSynced && (
              <span className="text-xs text-muted-foreground">
                {t('backends.gemini.lastSynced')}: {lastSynced}
              </span>
            )}
            {syncMutation.data && (
              <span className="text-xs text-status-success-fg">
                ✓ {syncMutation.data.count} {t('backends.gemini.globalModels').toLowerCase()}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-accent-gpu" />
          <h3 className="text-sm font-semibold text-text-bright">{t('backends.gemini.rateLimitPolicies')}</h3>
        </div>
        <p className="text-sm text-muted-foreground">
          {t('backends.gemini.rateLimitDesc')}
          {' '}{t('backends.gemini.globalFallbackHint')}
        </p>

        {tableLoading && (
          <div className="flex h-16 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!tableLoading && !hasContent && (
          <Card className="border-dashed">
            <CardContent className="p-6 text-center text-muted-foreground text-sm">
              {t('backends.gemini.noGlobalModels')}
            </CardContent>
          </Card>
        )}

        {!tableLoading && hasContent && (
          <Card>
            <CardContent className="p-0 overflow-x-auto">
              <Table className="min-w-[600px]">
                <TableHeader>
                  <TableRow className="border-b border-border hover:bg-transparent">
                    <TableHead className="text-muted-foreground font-semibold">{t('backends.gemini.model')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-36">{t('backends.gemini.onFreeTier')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-24 text-right">{t('backends.gemini.rpm')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-24 text-right">{t('backends.gemini.rpd')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-40">{t('backends.gemini.lastUpdated')}</TableHead>
                    <TableHead className="text-right text-muted-foreground font-semibold w-20">{t('common.edit')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {syncedRows.map((m) => {
                    const specific = policyMap.get(m.model_name)
                    const isInherited = !specific
                    const displayPolicy = specific ?? globalDefault
                    return (
                      <TableRow key={m.model_name} className={isInherited ? 'opacity-60' : ''}>
                        <TableCell className="py-3">
                          <span className="font-mono text-sm text-text-bright">{m.model_name}</span>
                        </TableCell>
                        <TableCell className="py-3">
                          {isInherited ? (
                            <span className="text-xs text-muted-foreground italic">{t('backends.gemini.globalDefault')}</span>
                          ) : displayPolicy?.available_on_free_tier ? (
                            <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                              {t('backends.gemini.enabled')}
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="bg-surface-code text-muted-foreground/70 border-border text-[10px] px-1.5 py-0">
                              {t('backends.gemini.paidOnly')}
                            </Badge>
                          )}
                        </TableCell>
                        <TableCell className="py-3 text-right tabular-nums font-mono text-sm">
                          {displayPolicy && displayPolicy.rpm_limit > 0
                            ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpm_limit}</span>
                            : <span className="text-text-faint">—</span>}
                        </TableCell>
                        <TableCell className="py-3 text-right tabular-nums font-mono text-sm">
                          {displayPolicy && displayPolicy.rpd_limit > 0
                            ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpd_limit}</span>
                            : <span className="text-text-faint">—</span>}
                        </TableCell>
                        <TableCell className="py-3 text-xs text-muted-foreground">
                          {specific?.updated_at ? fmtDate(specific.updated_at) : <span className="text-text-faint">—</span>}
                        </TableCell>
                        <TableCell className="py-3 text-right">
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-status-info-fg hover:bg-status-info/10"
                            onClick={() => setEditingPolicy(makeEditablePolicy(m.model_name))}
                            title={t('backends.gemini.editPolicyTitle')}>
                            <Pencil className="h-4 w-4" />
                          </Button>
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
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

// ── Tab: Ollama backends ───────────────────────────────────────────────────────

function OllamaTab({
  backends,
  servers,
  isLoading,
  error,
  onRegister,
  onEdit,
  onHealthcheck,
  healthcheckIsPending,
  onSyncModels,
  syncModelsPending,
  syncModelsVars,
  onDelete,
  deleteIsPending,
}: {
  backends: Backend[] | undefined
  servers: GpuServer[]
  isLoading: boolean
  error: Error | null
  onRegister: () => void
  onEdit: (b: Backend) => void
  onHealthcheck: (id: string) => void
  healthcheckIsPending: boolean
  onSyncModels: (id: string) => void
  syncModelsPending: boolean
  syncModelsVars: string | undefined
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const ollama = backends?.filter((b) => b.backend_type === 'ollama') ?? []
  const serverMap = new Map(servers.map((s) => [s.id, s]))
  const onlineCount   = ollama.filter((b) => b.status === 'online').length
  const offlineCount  = ollama.filter((b) => b.status === 'offline').length
  const degradedCount = ollama.filter((b) => b.status === 'degraded').length
  const [viewModelsBackend, setViewModelsBackend] = useState<Backend | null>(null)
  const [page, setPage] = useState(1)
  const totalPages = Math.max(1, Math.ceil(ollama.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages)
  const pageStart = (safePage - 1) * PAGE_SIZE
  const pageItems = ollama.slice(pageStart, pageStart + PAGE_SIZE)

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        {backends ? (
          <div className="flex items-center gap-2 flex-wrap">
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
              <Server className="h-3 w-3 shrink-0" />
              <span className="tabular-nums">{ollama.length}</span>
              <span>{t('backends.servers.registered')}</span>
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
          <Plus className="h-4 w-4 mr-2" />{t('backends.ollama.registerBackend')}
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground text-sm animate-pulse">
          {t('backends.ollama.loadingBackends')}
        </div>
      )}

      {error && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardContent className="p-5 text-destructive">
            <p className="font-semibold">{t('backends.ollama.failedBackends')}</p>
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
            <p className="font-medium">{t('backends.ollama.noBackends')}</p>
            <p className="text-sm mt-1">{t('backends.ollama.noBackendsHint')}</p>
          </CardContent>
        </Card>
      )}

      {ollama.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table className="min-w-[800px]">
              <TableHeader>
                <TableRow className="border-b border-border hover:bg-transparent">
                  <TableHead className="text-muted-foreground font-semibold">{t('backends.ollama.name')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold">{t('backends.ollama.server')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold w-28">{t('backends.ollama.status')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold w-32">{t('backends.servers.registeredAt')}</TableHead>
                  <TableHead className="text-right text-muted-foreground font-semibold w-36">{t('keys.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {pageItems.map((b) => {
                  const linkedServer = b.server_id ? serverMap.get(b.server_id) : null
                  return (
                    <TableRow key={b.id} className="align-top">
                      <TableCell className="pt-4 pb-4">
                        <div className="font-semibold text-text-bright mb-1">{b.name}</div>
                        {b.url && (
                          <span className="font-mono text-xs text-muted-foreground/70">{extractHost(b.url)}</span>
                        )}
                      </TableCell>

                      <TableCell className="pt-4 pb-4">
                        <div className="space-y-1 text-xs">
                          {linkedServer ? (
                            <div className="flex items-center gap-1.5 text-text-dim">
                              <Server className="h-3 w-3 text-muted-foreground/70 shrink-0" />
                              <span className="font-medium">{linkedServer.name}</span>
                            </div>
                          ) : (
                            <span className="text-text-faint italic text-xs">{t('backends.ollama.noServerLinked')}</span>
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
                              <span className="text-text-faint italic">{t('backends.servers.notConfigured')}</span>
                            )}
                          </div>
                          {linkedServer && (
                            <OllamaServerMetrics serverId={linkedServer.id} gpuIndex={b.gpu_index} />
                          )}
                        </div>
                      </TableCell>

                      <TableCell className="pt-4 pb-4">
                        <StatusBadge status={b.status} />
                      </TableCell>

                      <TableCell className="pt-4 pb-4 text-xs text-muted-foreground whitespace-nowrap">
                        {fmtDate(b.registered_at)}
                      </TableCell>

                      <TableCell className="pt-3 pb-4 text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-text-bright"
                            onClick={() => onHealthcheck(b.id)}
                            disabled={healthcheckIsPending}
                            title={t('backends.runHealthcheck')}>
                            <RefreshCw className="h-4 w-4" />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-text-bright"
                            onClick={() => onSyncModels(b.id)}
                            disabled={syncModelsPending && syncModelsVars === b.id}
                            title={t('backends.syncModelList')}>
                            <RotateCcw className={
                              syncModelsPending && syncModelsVars === b.id
                                ? 'h-4 w-4 animate-spin' : 'h-4 w-4'
                            } />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                            onClick={() => setViewModelsBackend(b)}
                            title={t('backends.ollama.viewModels')}>
                            <ListFilter className="h-4 w-4" />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-status-info-fg hover:bg-status-info/10"
                            onClick={() => onEdit(b)} title={t('backends.editBackend')}>
                            <Pencil className="h-4 w-4" />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                            onClick={() => onDelete(b.id, b.name)}
                            disabled={deleteIsPending} title={t('backends.removeBackend')}>
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
            {totalPages > 1 && (
              <div className="flex items-center justify-between px-4 py-2 border-t border-border">
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
            )}
          </CardContent>
        </Card>
      )}

      <OllamaSyncSection />

      {viewModelsBackend && (
        <OllamaBackendModelsModal
          backend={viewModelsBackend}
          onClose={() => setViewModelsBackend(null)}
        />
      )}
    </div>
  )
}

// ── Tab: Gemini backends + policies ───────────────────────────────────────────

function GeminiTab({
  backends,
  isLoading,
  error,
  onRegister,
  onEdit,
  onHealthcheck,
  healthcheckIsPending,
  onToggleActive,
  toggleActivePending,
  onDelete,
  deleteIsPending,
}: {
  backends: Backend[] | undefined
  isLoading: boolean
  error: Error | null
  onRegister: () => void
  onEdit: (b: Backend) => void
  onHealthcheck: (id: string) => void
  healthcheckIsPending: boolean
  onToggleActive: (b: Backend) => void
  toggleActivePending: boolean
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const gemini = backends?.filter((b) => b.backend_type === 'gemini') ?? []
  const onlineCount   = gemini.filter((b) => b.status === 'online').length
  const activeCount   = gemini.filter((b) => b.is_active).length
  const degradedCount = gemini.filter((b) => b.status === 'degraded').length
  const offlineCount  = gemini.filter((b) => b.status === 'offline').length
  const [modelSelectionBackend, setModelSelectionBackend] = useState<Backend | null>(null)
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
            <h2 className="text-base font-semibold text-text-bright">{t('backends.gemini.title')}</h2>
            {backends ? (
              <div className="flex items-center gap-2 flex-wrap mt-1.5">
                <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
                  <Key className="h-3 w-3 shrink-0" />
                  <span className="tabular-nums">{gemini.length}</span>
                  <span>{t('backends.servers.registered')}</span>
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
            <Plus className="h-4 w-4 mr-2" />{t('backends.gemini.registerBackend')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('backends.gemini.loadingBackends')}
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">{t('backends.gemini.failedBackends')}</p>
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
              <p className="font-medium text-text-dim">{t('backends.gemini.noBackends')}</p>
              <p className="text-sm mt-1 text-muted-foreground/70">{t('backends.gemini.noBackendsHint')}</p>
            </CardContent>
          </Card>
        )}

        {gemini.length > 0 && (
          <Card>
            <CardContent className="p-0 overflow-x-auto">
              <Table className="min-w-[760px]">
                <TableHeader>
                  <TableRow className="border-b border-border hover:bg-transparent">
                    <TableHead className="text-muted-foreground font-semibold">{t('backends.gemini.name')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold">{t('backends.gemini.apiKey')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-24">{t('backends.gemini.freeTier')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-24">{t('backends.gemini.activeToggle')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-28">{t('backends.gemini.status')}</TableHead>
                    <TableHead className="text-muted-foreground font-semibold w-32">{t('backends.servers.registeredAt')}</TableHead>
                    <TableHead className="text-right text-muted-foreground font-semibold w-28">{t('keys.actions')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {geminiPageItems.map((b) => (
                    <TableRow key={b.id} className={`align-top ${!b.is_active ? 'opacity-50' : ''}`}>
                      <TableCell className="pt-4 pb-4">
                        <div className="font-semibold text-text-bright">{b.name}</div>
                      </TableCell>
                      <TableCell className="pt-4 pb-4">
                        <ApiKeyCell backendId={b.id} masked={b.api_key_masked} />
                      </TableCell>
                      <TableCell className="pt-4 pb-4">
                        {b.is_free_tier ? (
                          <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                            {t('backends.gemini.freeTier')}
                          </Badge>
                        ) : (
                          <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 text-[10px] px-1.5 py-0">
                            {t('backends.gemini.paid')}
                          </Badge>
                        )}
                      </TableCell>
                      <TableCell className="pt-4 pb-4">
                        <Switch
                          checked={b.is_active}
                          onCheckedChange={() => onToggleActive(b)}
                          disabled={toggleActivePending}
                          title={b.is_active ? t('backends.disableBackend') : t('backends.enableBackend')}
                        />
                      </TableCell>
                      <TableCell className="pt-4 pb-4">
                        <StatusBadge status={b.status} />
                      </TableCell>
                      <TableCell className="pt-4 pb-4 text-xs text-muted-foreground whitespace-nowrap">
                        {fmtDate(b.registered_at)}
                      </TableCell>
                      <TableCell className="pt-3 pb-4 text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-text-bright"
                            onClick={() => onHealthcheck(b.id)}
                            disabled={healthcheckIsPending}
                            title={t('backends.runHealthcheck')}>
                            <RefreshCw className="h-4 w-4" />
                          </Button>
                          {!b.is_free_tier && (
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                              onClick={() => setModelSelectionBackend(b)}
                              title={t('backends.gemini.modelSelection')}>
                              <ListFilter className="h-4 w-4" />
                            </Button>
                          )}
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-status-info-fg hover:bg-status-info/10"
                            onClick={() => onEdit(b)} title={t('backends.editBackend')}>
                            <Pencil className="h-4 w-4" />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                            onClick={() => onDelete(b.id, b.name)}
                            disabled={deleteIsPending} title={t('backends.removeBackend')}>
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              {geminiTotalPages > 1 && (
                <div className="flex items-center justify-between px-4 py-2 border-t border-border">
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
              )}
            </CardContent>
          </Card>
        )}
      </div>

      <GeminiStatusSyncSection />

      <GeminiSyncSection />

      {modelSelectionBackend && (
        <ModelSelectionModal
          backend={modelSelectionBackend}
          onClose={() => setModelSelectionBackend(null)}
        />
      )}
    </div>
  )
}

// ── Page ──────────────────────────────────────────────────────────────────────

function ProvidersContent({ section }: { section: string }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const [registerBackendType, setRegisterBackendType] = useState<'ollama' | 'gemini' | null>(null)
  const [editingBackend, setEditingBackend] = useState<Backend | null>(null)

  // Servers needed for RegisterModal/EditModal dropdowns
  const { data: servers } = useQuery({
    queryKey: ['servers'],
    queryFn: () => api.servers(),
    staleTime: 60_000,
  })

  const { data: backends, isLoading: backendsLoading, error: backendsError } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    refetchInterval: 30_000,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const toggleActiveMutation = useMutation({
    mutationFn: (b: Backend) =>
      api.updateBackend(b.id, {
        name: b.name,
        is_active: !b.is_active,
        ...(b.backend_type === 'ollama' && { url: b.url, total_vram_mb: b.total_vram_mb, gpu_index: b.gpu_index, server_id: b.server_id }),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const healthcheckMutation = useMutation({
    mutationFn: (id: string) => api.healthcheckBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const syncModelsMutation = useMutation({
    mutationFn: (id: string) => api.syncBackendModels(id),
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: ['backend-models', id] })
      queryClient.invalidateQueries({ queryKey: ['selected-models', id] })
      queryClient.invalidateQueries({ queryKey: ['ollama-sync-status'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
    },
  })

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('backends.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('backends.description')}</p>
      </div>

      {section === 'ollama' && (
        <OllamaTab
          backends={backends}
          servers={servers ?? []}
          isLoading={backendsLoading}
          error={backendsError as Error | null}
          onRegister={() => setRegisterBackendType('ollama')}
          onEdit={(b) => setEditingBackend(b)}
          onHealthcheck={(id) => healthcheckMutation.mutate(id)}
          healthcheckIsPending={healthcheckMutation.isPending}
          onSyncModels={(id) => syncModelsMutation.mutate(id)}
          syncModelsPending={syncModelsMutation.isPending}
          syncModelsVars={syncModelsMutation.variables}
          onDelete={(id, name) => { if (confirm(t('backends.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {section === 'gemini' && (
        <GeminiTab
          backends={backends}
          isLoading={backendsLoading}
          error={backendsError as Error | null}
          onRegister={() => setRegisterBackendType('gemini')}
          onEdit={(b) => setEditingBackend(b)}
          onHealthcheck={(id) => healthcheckMutation.mutate(id)}
          healthcheckIsPending={healthcheckMutation.isPending}
          onToggleActive={(b) => toggleActiveMutation.mutate(b)}
          toggleActivePending={toggleActiveMutation.isPending}
          onDelete={(id, name) => { if (confirm(t('backends.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {registerBackendType && (
        <RegisterModal
          servers={servers ?? []}
          initialType={registerBackendType}
          onClose={() => setRegisterBackendType(null)}
        />
      )}
      {editingBackend && (
        <EditModal
          backend={editingBackend}
          servers={servers ?? []}
          onClose={() => setEditingBackend(null)}
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
