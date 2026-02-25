'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Backend, GeminiRateLimitPolicy, GpuServer, NodeMetrics, RegisterBackendRequest, RegisterGpuServerRequest, ServerMetricsPoint, UpdateBackendRequest } from '@/lib/types'
import { Plus, Trash2, RefreshCw, RotateCcw, Server, Key, Wifi, WifiOff, AlertCircle, Thermometer, Zap, BarChart2, Pencil, MemoryStick, ShieldCheck } from 'lucide-react'
import {
  LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer,
} from 'recharts'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
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
  if (status === 'online') return (
    <Badge variant="outline" className="bg-emerald-500/15 text-emerald-400 border-emerald-500/30 font-medium">
      <Wifi className="h-3 w-3 mr-1.5" />online
    </Badge>
  )
  if (status === 'degraded') return (
    <Badge variant="outline" className="bg-amber-500/15 text-amber-400 border-amber-500/30 font-medium">
      <AlertCircle className="h-3 w-3 mr-1.5" />degraded
    </Badge>
  )
  return (
    <Badge variant="outline" className="bg-slate-800 text-slate-400 border-slate-700 font-medium">
      <WifiOff className="h-3 w-3 mr-1.5" />offline
    </Badge>
  )
}

// ── Live metrics cell ──────────────────────────────────────────────────────────

function ServerMetricsCell({ serverId }: { serverId: string }) {
  const { data, isLoading, isError, refetch, isFetching } = useQuery<NodeMetrics>({
    queryKey: ['server-metrics', serverId],
    queryFn: () => api.serverMetrics(serverId),
    refetchInterval: 30_000,
    retry: false,
  })

  if (isLoading) {
    return <span className="text-xs text-muted-foreground animate-pulse">fetching…</span>
  }

  if (isError || !data || !data.scrape_ok) {
    return (
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="bg-red-500/10 text-red-400 border-red-500/30 text-xs font-medium">
          <WifiOff className="h-3 w-3 mr-1.5" />unreachable
        </Badge>
        <Button variant="ghost" size="icon" className="h-6 w-6 text-muted-foreground hover:text-foreground"
          onClick={() => refetch()} disabled={isFetching} title="Retry">
          <RefreshCw className={isFetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
        </Button>
      </div>
    )
  }

  const memUsed = data.mem_total_mb - data.mem_available_mb
  const memPct = data.mem_total_mb > 0
    ? Math.round((memUsed / data.mem_total_mb) * 100) : 0

  return (
    <div className="space-y-2 text-xs">
      {/* Memory row */}
      <div className="flex items-center gap-2">
        <span className="w-6 text-[10px] font-semibold text-slate-500 uppercase tracking-wide shrink-0">MEM</span>
        <span className="text-slate-200 font-mono tabular-nums">
          {fmtMb(memUsed)}<span className="text-slate-500"> / {fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`ml-auto font-semibold tabular-nums ${memPct >= 90 ? 'text-red-400' : memPct >= 75 ? 'text-amber-400' : 'text-slate-400'}`}>
          {memPct}%
        </span>
      </div>

      {/* CPU row */}
      {data.cpu_cores > 0 && (
        <div className="flex items-center gap-2">
          <span className="w-6 text-[10px] font-semibold text-slate-500 uppercase tracking-wide shrink-0">CPU</span>
          <span className="text-slate-300 tabular-nums">{data.cpu_cores} cores</span>
        </div>
      )}

      {/* GPU rows */}
      {data.gpus.map((gpu) => (
        <div key={gpu.card} className="flex items-center gap-2 flex-wrap">
          <span className="w-6 text-[10px] font-semibold text-violet-400 uppercase tracking-wide shrink-0">GPU</span>
          <span className="text-slate-300 font-mono">{gpu.card}</span>
          {gpu.temp_c != null && (
            <span className={`flex items-center gap-0.5 tabular-nums ${gpu.temp_c >= 85 ? 'text-red-400 font-bold' : 'text-slate-300'}`}>
              <Thermometer className="h-3 w-3" />{gpu.temp_c.toFixed(0)}°C
            </span>
          )}
          {gpu.power_w != null && (
            <span className="flex items-center gap-0.5 text-slate-300 tabular-nums">
              <Zap className="h-3 w-3 text-yellow-500" />{gpu.power_w.toFixed(0)}W
            </span>
          )}
          {gpu.vram_total_mb != null && (
            <span className="flex items-center gap-0.5 text-slate-400 tabular-nums">
              <MemoryStick className="h-3 w-3" />{fmtMb(gpu.vram_used_mb ?? 0)}/{fmtMb(gpu.vram_total_mb)}
            </span>
          )}
          {gpu.busy_pct != null && (
            <span className="text-slate-400 tabular-nums">{gpu.busy_pct.toFixed(0)}%</span>
          )}
        </div>
      ))}
    </div>
  )
}

// ── ClickHouse history modal ───────────────────────────────────────────────────

const HIST_HOUR_OPTIONS = [1, 3, 6, 24] as const

function fmtTs(iso: string) {
  const d = new Date(iso)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

function ServerHistoryModal({ server, onClose }: { server: GpuServer; onClose: () => void }) {
  const [hours, setHours] = useState<1 | 3 | 6 | 24>(1)

  const { data, isLoading, isError, refetch, isFetching } = useQuery<ServerMetricsPoint[]>({
    queryKey: ['server-metrics-history', server.id, hours],
    queryFn: () => api.serverMetricsHistory(server.id, hours),
    staleTime: 0,
  })

  const chartData = (data ?? []).map((p) => ({
    ts: fmtTs(p.ts),
    memUsedPct: p.mem_total_mb > 0
      ? Math.round(((p.mem_total_mb - p.mem_avail_mb) / p.mem_total_mb) * 100) : 0,
    gpuTemp: p.gpu_temp_c ?? undefined,
    gpuPower: p.gpu_power_w !== null ? Math.round((p.gpu_power_w ?? 0) * 10) / 10 : undefined,
  }))

  const hasGpu = (data ?? []).some((p) => p.gpu_temp_c !== null || p.gpu_power_w !== null)

  const tooltipStyle = {
    background: 'var(--theme-bg-card)',
    border: '1px solid var(--theme-border)',
    borderRadius: 6,
    fontSize: 11,
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <BarChart2 className="h-4 w-4 text-violet-400" />
            {server.name}
            <span className="text-muted-foreground font-normal text-sm">— ClickHouse History</span>
          </DialogTitle>
        </DialogHeader>

        <div className="flex items-center gap-2 border-b border-border pb-3">
          <div className="flex items-center gap-1 bg-muted rounded-md p-0.5">
            {HIST_HOUR_OPTIONS.map((h) => (
              <Button key={h} size="sm" variant={hours === h ? 'default' : 'ghost'}
                onClick={() => setHours(h as typeof hours)}
                className="h-6 px-3 text-xs rounded">
                {h}h
              </Button>
            ))}
          </div>
          <Button size="sm" variant="ghost" onClick={() => refetch()} disabled={isFetching}
            className="h-7 px-2 ml-auto gap-1.5 text-xs text-muted-foreground hover:text-foreground">
            <RefreshCw className={isFetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
            Sync
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            Loading from ClickHouse…
          </div>
        )}
        {isError && (
          <p className="text-sm text-destructive py-2">
            Failed to load ClickHouse data. Check that ClickHouse is reachable.
          </p>
        )}
        {data && data.length === 0 && (
          <p className="text-sm text-muted-foreground py-6 text-center">
            No data in ClickHouse for the last {hours}h.
            <br />
            <span className="text-xs opacity-60">Check the OTel Collector pipeline and node-exporter endpoint.</span>
          </p>
        )}

        {data && data.length > 0 && (
          <div className="space-y-5">
            <div>
              <p className="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-2">Memory Used %</p>
              <ResponsiveContainer width="100%" height={110}>
                <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                  <XAxis dataKey="ts" tick={{ fontSize: 10, fill: '#94a3b8' }} interval="preserveStartEnd" />
                  <YAxis domain={[0, 100]} tick={{ fontSize: 10, fill: '#94a3b8' }} unit="%" />
                  <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}%`, 'Mem Used']} />
                  <Line type="monotone" dataKey="memUsedPct" stroke="#3b82f6" dot={false} strokeWidth={2} />
                </LineChart>
              </ResponsiveContainer>
            </div>

            {hasGpu && (
              <>
                <div>
                  <p className="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-2">GPU Temperature (°C)</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={{ fontSize: 10, fill: '#94a3b8' }} interval="preserveStartEnd" />
                      <YAxis tick={{ fontSize: 10, fill: '#94a3b8' }} unit="°" />
                      <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}°C`, 'GPU Temp']} />
                      <Line type="monotone" dataKey="gpuTemp" stroke="#f97316" dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>

                <div>
                  <p className="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-2">GPU Power (W)</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={{ fontSize: 10, fill: '#94a3b8' }} interval="preserveStartEnd" />
                      <YAxis tick={{ fontSize: 10, fill: '#94a3b8' }} unit="W" />
                      <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}W`, 'GPU Power']} />
                      <Line type="monotone" dataKey="gpuPower" stroke="#a855f7" dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </>
            )}

            <p className="text-xs text-slate-500 text-right">
              {data.length} points · 1-min buckets · OTel → ClickHouse
            </p>
          </div>
        )}
      </DialogContent>
    </Dialog>
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
          <DialogTitle>Edit Backend</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-name">Name <span className="text-destructive">*</span></Label>
            <Input id="edit-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          {backend.backend_type === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="edit-url">Ollama URL</Label>
                <Input id="edit-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="edit-server">
                  GPU Server <span className="text-muted-foreground font-normal">— optional</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="edit-server"><SelectValue placeholder="None" /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">None</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-1.5">
                <Label>GPU Index <span className="text-muted-foreground font-normal">— optional</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder="None" /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">None</SelectItem>
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
                    placeholder="0 — select a GPU server above to see options" />
                )}
              </div>

              <div className="space-y-1.5">
                <Label>Max VRAM <span className="text-muted-foreground font-normal">— optional</span></Label>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>
            </>
          )}

          {backend.backend_type === 'gemini' && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="edit-apikey">
                  Gemini API Key <span className="text-muted-foreground font-normal">— leave blank to keep existing</span>
                </Label>
                <Input id="edit-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza… (blank = keep current)" />
                <p className="text-xs text-muted-foreground">
                  Rate limits are per Google project. Use keys from different accounts for rolling.
                </p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">Free Tier</p>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    Mark as Google free-tier project — rate limits are managed globally in the Gemini Policies section below.
                  </p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to update backend'}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? 'Saving…' : 'Save changes'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Register GPU Server modal ──────────────────────────────────────────────────

function RegisterServerModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('')
  const [nodeExporterUrl, setNodeExporterUrl] = useState('')
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: RegisterGpuServerRequest = {
        name: name.trim(),
        node_exporter_url: nodeExporterUrl.trim() || undefined,
      }
      return api.registerServer(body)
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['servers'] }); onClose() },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Register GPU Server</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="server-name">Name <span className="text-destructive">*</span></Label>
            <Input id="server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder="e.g. gpu-node-1" />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="server-ne-url">
              node-exporter URL <span className="text-muted-foreground font-normal">— optional</span>
            </Label>
            <Input id="server-ne-url" type="url" value={nodeExporterUrl}
              onChange={(e) => setNodeExporterUrl(e.target.value)}
              placeholder="http://192.168.1.10:9100" />
            <p className="text-xs text-muted-foreground">
              CPU, memory, and GPU metrics are scraped from this endpoint (live + OTel pipeline).
            </p>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to register server'}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? 'Registering…' : 'Register'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Register backend modal ─────────────────────────────────────────────────────

function RegisterModal({ servers, onClose }: { servers: GpuServer[]; onClose: () => void }) {
  const [backendType, setBackendType] = useState<'ollama' | 'gemini'>('ollama')
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
        backend_type: backendType,
        ...(backendType === 'ollama' && {
          url: url.trim(),
          total_vram_mb: vram ? parseInt(vram, 10) : undefined,
          gpu_index: gpuIndex !== 'none' && gpuIndex !== '' ? parseInt(gpuIndex, 10) : undefined,
          server_id: serverId !== 'none' ? serverId : undefined,
        }),
        ...(backendType === 'gemini' && {
          api_key: apiKey.trim(),
          is_free_tier: isFreeTier,
        }),
      }
      return api.registerBackend(body)
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['backends'] }); onClose() },
  })

  const isValid = name.trim() && (backendType === 'ollama' ? url.trim() : apiKey.trim())

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Register Backend</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label>Type</Label>
            <div className="grid grid-cols-2 gap-2">
              {(['ollama', 'gemini'] as const).map((t) => (
                <Button key={t} type="button" variant={backendType === t ? 'default' : 'outline'}
                  onClick={() => setBackendType(t)}
                  className="flex items-center justify-center gap-2">
                  {t === 'ollama' ? <Server className="h-4 w-4" /> : <Key className="h-4 w-4" />}
                  {t === 'ollama' ? 'Ollama Server' : 'Gemini API'}
                </Button>
              ))}
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="backend-name">Name <span className="text-destructive">*</span></Label>
            <Input id="backend-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={backendType === 'ollama' ? 'e.g. gpu-server-1' : 'e.g. gemini-prod'} />
          </div>

          {backendType === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="backend-url">Ollama URL <span className="text-destructive">*</span></Label>
                <Input id="backend-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://192.168.1.10:11434" />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="backend-server">
                  GPU Server <span className="text-muted-foreground font-normal">— optional</span>
                </Label>
                <Select value={serverId} onValueChange={setServerId}>
                  <SelectTrigger id="backend-server"><SelectValue placeholder="None" /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">None</SelectItem>
                    {servers.map((s) => (
                      <SelectItem key={s.id} value={s.id}>
                        {s.name}{s.node_exporter_url ? ` (${extractHost(s.node_exporter_url)})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">Link to a GPU server for metric correlation.</p>
              </div>

              <div className="space-y-1.5">
                <Label>GPU Index <span className="text-muted-foreground font-normal">— optional</span></Label>
                {gpuCards.length > 0 ? (
                  <Select value={gpuIndex} onValueChange={setGpuIndex}>
                    <SelectTrigger><SelectValue placeholder="None" /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">None</SelectItem>
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
                    placeholder="0 — select a GPU server above to see options" />
                )}
              </div>

              <div className="space-y-1.5">
                <Label>Max VRAM <span className="text-muted-foreground font-normal">— optional</span></Label>
                <VramInput valueMb={vram} onChange={setVram} />
              </div>
            </>
          )}

          {backendType === 'gemini' && (
            <div className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="backend-apikey">Gemini API Key <span className="text-destructive">*</span></Label>
                <Input id="backend-apikey" type="password" value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)} placeholder="AIza…" />
                <p className="text-xs text-muted-foreground">
                  Rate limits are per Google project, not per key. Use keys from different accounts for rolling.
                </p>
              </div>

              <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">Free Tier</p>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    Mark as Google free-tier project — rate limits are managed globally in the Gemini Policies section.
                  </p>
                </div>
                <Switch checked={isFreeTier} onCheckedChange={setIsFreeTier} />
              </div>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to register backend'}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={() => mutation.mutate()} disabled={!isValid || mutation.isPending}>
            {mutation.isPending ? 'Registering…' : 'Register'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Gemini Policies section ────────────────────────────────────────────────────

function EditPolicyModal({
  policy,
  onClose,
}: {
  policy: GeminiRateLimitPolicy
  onClose: () => void
}) {
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['gemini-policies'] })
      onClose()
    },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-purple-400" />
            Edit Rate Limit Policy
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-1 mb-1">
          <p className="text-sm text-muted-foreground">Model</p>
          <p className="font-mono text-sm font-semibold text-slate-100">
            {policy.model_name === '*' ? '* (global default)' : policy.model_name}
          </p>
        </div>

        <div className="space-y-4">
          {/* Free tier availability toggle */}
          <div className="flex items-center justify-between rounded-lg border border-border px-4 py-3">
            <div>
              <p className="text-sm font-medium">Available on Free Tier</p>
              <p className="text-xs text-muted-foreground mt-0.5">
                {availableOnFreeTier
                  ? 'Routes to free-tier backends first, enforces RPM/RPD.'
                  : 'Skips free-tier backends — routes directly to paid. No counter increment.'}
              </p>
            </div>
            <Switch checked={availableOnFreeTier} onCheckedChange={setAvailableOnFreeTier} />
          </div>

          {/* RPM / RPD limits (only relevant when free tier is enabled) */}
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
                0 = no enforcement. 2026 free limits: 2.5-pro 5/100 · 2.5-flash 10/250 · 2.5-flash-lite 15/1000
              </p>
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to save'}
          </p>
        )}
        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function GeminiPoliciesSection() {
  const [editingPolicy, setEditingPolicy] = useState<GeminiRateLimitPolicy | null>(null)

  const { data: policies, isLoading } = useQuery({
    queryKey: ['gemini-policies'],
    queryFn: () => api.geminiPolicies(),
    refetchInterval: 60_000,
  })

  return (
    <section className="space-y-4">
      <div>
        <h2 className="text-xl font-bold text-slate-100 tracking-tight flex items-center gap-2">
          <ShieldCheck className="h-5 w-5 text-purple-400" />
          Gemini Rate Limit Policies
        </h2>
        <p className="text-sm text-slate-400 mt-0.5">
          Shared RPM/RPD limits per model, applied to all free-tier backends.
          {' '}Use <code className="text-[11px] bg-slate-800 px-1 py-0.5 rounded">*</code> as a global fallback for models without a specific row.
        </p>
      </div>

      {isLoading && (
        <div className="flex h-16 items-center justify-center text-muted-foreground text-sm animate-pulse">
          Loading policies…
        </div>
      )}

      {policies && policies.length === 0 && (
        <Card className="border-dashed">
          <CardContent className="p-6 text-center text-muted-foreground text-sm">
            No policies configured. Run the migration to seed defaults.
          </CardContent>
        </Card>
      )}

      {policies && policies.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="border-b border-border hover:bg-transparent">
                  <TableHead className="text-slate-400 font-semibold">Model</TableHead>
                  <TableHead className="text-slate-400 font-semibold w-28 text-right">RPM</TableHead>
                  <TableHead className="text-slate-400 font-semibold w-28 text-right">RPD</TableHead>
                  <TableHead className="text-slate-400 font-semibold w-40">Last Updated</TableHead>
                  <TableHead className="text-right text-slate-400 font-semibold w-20">Edit</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {policies.map((p) => (
                  <TableRow key={p.id}>
                    <TableCell className="py-3">
                      <span className="font-mono text-sm text-slate-100">
                        {p.model_name === '*'
                          ? <span className="flex items-center gap-1.5"><span className="text-slate-400">* </span><span className="text-xs text-muted-foreground font-sans">global default</span></span>
                          : p.model_name}
                      </span>
                    </TableCell>
                    <TableCell className="py-3 text-right tabular-nums font-mono text-sm">
                      {p.rpm_limit > 0 ? p.rpm_limit : <span className="text-slate-600">—</span>}
                    </TableCell>
                    <TableCell className="py-3 text-right tabular-nums font-mono text-sm">
                      {p.rpd_limit > 0 ? p.rpd_limit : <span className="text-slate-600">—</span>}
                    </TableCell>
                    <TableCell className="py-3 text-xs text-slate-400">
                      {fmtDate(p.updated_at)}
                    </TableCell>
                    <TableCell className="py-3 text-right">
                      <Button variant="ghost" size="icon"
                        className="h-8 w-8 text-slate-400 hover:text-blue-400 hover:bg-blue-500/10"
                        onClick={() => setEditingPolicy(p)} title="Edit limits">
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {editingPolicy && (
        <EditPolicyModal policy={editingPolicy} onClose={() => setEditingPolicy(null)} />
      )}
    </section>
  )
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function BackendsPage() {
  const queryClient = useQueryClient()
  const [showRegisterServer, setShowRegisterServer] = useState(false)
  const [showRegisterBackend, setShowRegisterBackend] = useState(false)
  const [editingBackend, setEditingBackend] = useState<Backend | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)

  const { data: servers, isLoading: serversLoading } = useQuery({
    queryKey: ['servers'],
    queryFn: () => api.servers(),
    refetchInterval: 30_000,
  })

  const { data: backends, isLoading: backendsLoading, error } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    refetchInterval: 30_000,
  })

  const deleteServerMutation = useMutation({
    mutationFn: (id: string) => api.deleteServer(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['servers'] }),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const healthcheckMutation = useMutation({
    mutationFn: (id: string) => api.healthcheckBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const syncModelsMutation = useMutation({
    mutationFn: (id: string) => api.syncBackendModels(id),
    onSuccess: (_data, id) => queryClient.invalidateQueries({ queryKey: ['backend-models', id] }),
  })

  const serverMap = new Map((servers ?? []).map((s) => [s.id, s]))

  const ollamaCount = backends?.filter((b) => b.backend_type === 'ollama').length ?? 0
  const geminiCount = backends?.filter((b) => b.backend_type === 'gemini').length ?? 0
  const onlineCount = backends?.filter((b) => b.status === 'online').length ?? 0

  return (
    <div className="space-y-10">

      {/* ── GPU Servers ────────────────────────────────────────────────── */}
      <section className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-xl font-bold text-slate-100 tracking-tight">GPU Servers</h1>
            <p className="text-sm text-slate-400 mt-0.5">
              Physical machines — one node-exporter per server.
              {servers ? ` ${servers.length} registered.` : ''}
            </p>
          </div>
          <Button onClick={() => setShowRegisterServer(true)} className="shrink-0">
            <Plus className="h-4 w-4 mr-2" />Register GPU Server
          </Button>
        </div>

        {serversLoading && (
          <div className="flex h-24 items-center justify-center text-muted-foreground text-sm animate-pulse">
            Loading servers…
          </div>
        )}

        {servers && servers.length === 0 && (
          <Card className="border-dashed">
            <CardContent className="p-8 text-center text-muted-foreground">
              <Server className="h-8 w-8 mx-auto mb-3 opacity-25" />
              <p className="font-medium text-slate-300">No GPU servers registered</p>
              <p className="text-sm mt-1 text-slate-500">
                Register a server to enable node-exporter metric collection.
              </p>
            </CardContent>
          </Card>
        )}

        {servers && servers.length > 0 && (
          <Card>
            <CardContent className="p-0 overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow className="border-b border-border hover:bg-transparent">
                    <TableHead className="w-48 text-slate-400 font-semibold">Name</TableHead>
                    <TableHead className="text-slate-400 font-semibold">node-exporter endpoint</TableHead>
                    <TableHead className="text-slate-400 font-semibold min-w-64">Live Metrics</TableHead>
                    <TableHead className="text-slate-400 font-semibold w-32">Registered</TableHead>
                    <TableHead className="text-right text-slate-400 font-semibold w-24">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {servers.map((s) => (
                    <TableRow key={s.id} className="align-top">
                      <TableCell className="pt-4 pb-4 font-semibold text-slate-100">{s.name}</TableCell>
                      <TableCell className="pt-4 pb-4">
                        {s.node_exporter_url
                          ? <span className="font-mono text-xs text-slate-300 bg-slate-800 px-2 py-1 rounded">{s.node_exporter_url}</span>
                          : <span className="text-xs text-slate-600 italic">not configured</span>
                        }
                      </TableCell>
                      <TableCell className="pt-4 pb-4">
                        {s.node_exporter_url
                          ? <ServerMetricsCell serverId={s.id} />
                          : <span className="text-xs text-slate-600 italic">—</span>
                        }
                      </TableCell>
                      <TableCell className="pt-4 pb-4 text-slate-400 text-xs whitespace-nowrap">
                        {fmtDate(s.registered_at)}
                      </TableCell>
                      <TableCell className="pt-3 pb-4 text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-slate-400 hover:text-violet-400 hover:bg-violet-500/10"
                            onClick={() => setHistoryServer(s)} title="ClickHouse history">
                            <BarChart2 className="h-4 w-4" />
                          </Button>
                          <Button variant="ghost" size="icon"
                            className="h-8 w-8 text-slate-400 hover:text-red-400 hover:bg-red-500/10"
                            onClick={() => { if (confirm(`Remove GPU server "${s.name}"?`)) deleteServerMutation.mutate(s.id) }}
                            disabled={deleteServerMutation.isPending} title="Remove server">
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        )}
      </section>

      {/* ── LLM Backends ──────────────────────────────────────────────── */}
      <section className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-xl font-bold text-slate-100 tracking-tight">LLM Backends</h2>
            <p className="text-sm text-slate-400 mt-0.5">
              {backends
                ? `${backends.length} registered — ${ollamaCount} Ollama, ${geminiCount} Gemini — ${onlineCount} online`
                : 'Loading…'}
            </p>
          </div>
          <Button onClick={() => setShowRegisterBackend(true)} className="shrink-0">
            <Plus className="h-4 w-4 mr-2" />Register Backend
          </Button>
        </div>

        {backendsLoading && (
          <div className="flex h-48 items-center justify-center text-muted-foreground text-sm animate-pulse">
            Loading backends…
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">Failed to load backends</p>
              <p className="text-sm mt-1 opacity-75">
                {error instanceof Error ? error.message : 'Unknown error'}
              </p>
            </CardContent>
          </Card>
        )}

        {backends && backends.length === 0 && (
          <Card className="border-dashed">
            <CardContent className="p-10 text-center text-muted-foreground">
              <Server className="h-10 w-10 mx-auto mb-3 opacity-25" />
              <p className="font-medium text-slate-300">No backends registered</p>
              <p className="text-sm mt-1 text-slate-500">
                Add an Ollama server or Gemini API key to start routing inference.
              </p>
            </CardContent>
          </Card>
        )}

        {backends && backends.length > 0 && (
          <Card>
            <CardContent className="p-0 overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow className="border-b border-border hover:bg-transparent">
                    <TableHead className="text-slate-400 font-semibold">Backend</TableHead>
                    <TableHead className="text-slate-400 font-semibold">Assignment</TableHead>
                    <TableHead className="text-slate-400 font-semibold w-28">Status</TableHead>
                    <TableHead className="text-slate-400 font-semibold w-32">Registered</TableHead>
                    <TableHead className="text-right text-slate-400 font-semibold w-36">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {backends.map((b) => {
                    const linkedServer = b.server_id ? serverMap.get(b.server_id) : null
                    return (
                      <TableRow key={b.id} className="align-top">
                        {/* Backend: name + type + URL */}
                        <TableCell className="pt-4 pb-4">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="font-semibold text-slate-100">{b.name}</span>
                            <Badge variant="outline" className={
                              b.backend_type === 'ollama'
                                ? 'bg-blue-500/15 text-blue-300 border-blue-500/30 text-[10px] px-1.5 py-0'
                                : 'bg-purple-500/15 text-purple-300 border-purple-500/30 text-[10px] px-1.5 py-0'
                            }>
                              {b.backend_type === 'ollama'
                                ? <Server className="h-2.5 w-2.5 mr-1" />
                                : <Key className="h-2.5 w-2.5 mr-1" />}
                              {b.backend_type}
                            </Badge>
                          </div>
                          {b.backend_type === 'ollama' && b.url && (
                            <span className="font-mono text-xs text-slate-500">{extractHost(b.url)}</span>
                          )}
                        </TableCell>

                        {/* Assignment: server + GPU + VRAM */}
                        <TableCell className="pt-4 pb-4">
                          {b.backend_type === 'ollama' ? (
                            <div className="space-y-1 text-xs">
                              {linkedServer ? (
                                <div className="flex items-center gap-1.5 text-slate-300">
                                  <Server className="h-3 w-3 text-slate-500 shrink-0" />
                                  <span className="font-medium">{linkedServer.name}</span>
                                </div>
                              ) : (
                                <span className="text-slate-600 italic text-xs">no server linked</span>
                              )}
                              <div className="flex items-center gap-3 text-slate-400 pl-0.5">
                                {b.gpu_index !== null && (
                                  <span className="flex items-center gap-1">
                                    <span className="text-[10px] font-semibold text-slate-500 uppercase">GPU</span>
                                    <span className="tabular-nums font-mono">{b.gpu_index}</span>
                                  </span>
                                )}
                                {b.total_vram_mb > 0 && (
                                  <span className="flex items-center gap-1">
                                    <span className="text-[10px] font-semibold text-slate-500 uppercase">VRAM</span>
                                    <span className="tabular-nums font-mono">{fmtMb(b.total_vram_mb)}</span>
                                  </span>
                                )}
                                {b.gpu_index === null && b.total_vram_mb === 0 && linkedServer && (
                                  <span className="text-slate-600 italic">not configured</span>
                                )}
                              </div>
                            </div>
                          ) : (
                            <div className="text-xs">
                              {b.is_free_tier ? (
                                <Badge variant="outline" className="bg-amber-500/15 text-amber-400 border-amber-500/30 text-[10px] px-1.5 py-0">
                                  Free Tier
                                </Badge>
                              ) : (
                                <Badge variant="outline" className="bg-emerald-500/15 text-emerald-400 border-emerald-500/30 text-[10px] px-1.5 py-0">
                                  Paid
                                </Badge>
                              )}
                            </div>
                          )}
                        </TableCell>

                        {/* Status */}
                        <TableCell className="pt-4 pb-4">
                          <StatusBadge status={b.status} />
                        </TableCell>

                        {/* Registered */}
                        <TableCell className="pt-4 pb-4 text-xs text-slate-400 whitespace-nowrap">
                          {fmtDate(b.registered_at)}
                        </TableCell>

                        {/* Actions */}
                        <TableCell className="pt-3 pb-4 text-right">
                          <div className="flex items-center justify-end gap-1">
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-slate-400 hover:text-slate-100"
                              onClick={() => healthcheckMutation.mutate(b.id)}
                              disabled={healthcheckMutation.isPending}
                              title="Run health check">
                              <RefreshCw className="h-4 w-4" />
                            </Button>
                            {b.backend_type === 'ollama' && (
                              <Button variant="ghost" size="icon"
                                className="h-8 w-8 text-slate-400 hover:text-slate-100"
                                onClick={() => syncModelsMutation.mutate(b.id)}
                                disabled={syncModelsMutation.isPending && syncModelsMutation.variables === b.id}
                                title="Sync model list">
                                <RotateCcw className={
                                  syncModelsMutation.isPending && syncModelsMutation.variables === b.id
                                    ? 'h-4 w-4 animate-spin' : 'h-4 w-4'
                                } />
                              </Button>
                            )}
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-slate-400 hover:text-blue-400 hover:bg-blue-500/10"
                              onClick={() => setEditingBackend(b)} title="Edit backend">
                              <Pencil className="h-4 w-4" />
                            </Button>
                            <Button variant="ghost" size="icon"
                              className="h-8 w-8 text-slate-400 hover:text-red-400 hover:bg-red-500/10"
                              onClick={() => { if (confirm(`Remove backend "${b.name}"?`)) deleteMutation.mutate(b.id) }}
                              disabled={deleteMutation.isPending} title="Remove backend">
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        )}
      </section>

      {/* ── Gemini Rate Limit Policies ─────────────────────────────── */}
      <GeminiPoliciesSection />

      {/* Modals */}
      {showRegisterServer && <RegisterServerModal onClose={() => setShowRegisterServer(false)} />}
      {showRegisterBackend && <RegisterModal servers={servers ?? []} onClose={() => setShowRegisterBackend(false)} />}
      {editingBackend && <EditModal backend={editingBackend} servers={servers ?? []} onClose={() => setEditingBackend(null)} />}
      {historyServer && <ServerHistoryModal server={historyServer} onClose={() => setHistoryServer(null)} />}
    </div>
  )
}
