'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { GpuServer, NodeMetrics, ServerMetricsPoint, RegisterGpuServerRequest, UpdateGpuServerRequest } from '@/lib/types'
import {
  Plus, Trash2, RefreshCw, BarChart2, Pencil,
  Server, Thermometer, Zap, MemoryStick, WifiOff, HardDrive,
  ChevronLeft, ChevronRight,
} from 'lucide-react'
import {
  LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer,
} from 'recharts'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
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
import { useTranslation } from '@/i18n'

// ── Helpers ────────────────────────────────────────────────────────────────────

function fmtMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

// ── Live metrics cell ──────────────────────────────────────────────────────────

function ServerMetricsCell({ serverId }: { serverId: string }) {
  const { t } = useTranslation()
  const { data, isLoading, isError, refetch, isFetching } = useQuery<NodeMetrics>({
    queryKey: ['server-metrics', serverId],
    queryFn: () => api.serverMetrics(serverId),
    refetchInterval: 30_000,
    retry: false,
  })

  if (isLoading) {
    return <span className="text-xs text-muted-foreground animate-pulse">{t('common.loading')}</span>
  }

  if (isError || !data || !data.scrape_ok) {
    return (
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="bg-status-error/10 text-status-error-fg border-status-error/30 text-xs font-medium">
          <WifiOff className="h-3 w-3 mr-1.5" />{t('backends.servers.unreachable')}
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
      <div className="flex items-center gap-2">
        <span className="w-6 text-[10px] font-semibold text-muted-foreground/70 uppercase tracking-wide shrink-0">MEM</span>
        <span className="text-text-bright font-mono tabular-nums">
          {fmtMb(memUsed)}<span className="text-muted-foreground/70"> / {fmtMb(data.mem_total_mb)}</span>
        </span>
        <span className={`ml-auto font-semibold tabular-nums ${memPct >= 90 ? 'text-status-error-fg' : memPct >= 75 ? 'text-status-warning-fg' : 'text-muted-foreground'}`}>
          {memPct}%
        </span>
      </div>
      {data.cpu_cores > 0 && (
        <div className="flex items-center gap-2">
          <span className="w-6 text-[10px] font-semibold text-muted-foreground/70 uppercase tracking-wide shrink-0">CPU</span>
          <span className="text-text-dim tabular-nums">{data.cpu_cores}</span>
        </div>
      )}
      {data.gpus.map((gpu) => (
        <div key={gpu.card} className="flex items-center gap-2 flex-wrap">
          <span className="w-6 text-[10px] font-semibold text-accent-gpu uppercase tracking-wide shrink-0">GPU</span>
          <span className="text-text-dim font-mono">{gpu.card}</span>
          {gpu.temp_c != null && (
            <span className={`flex items-center gap-0.5 tabular-nums ${gpu.temp_c >= 85 ? 'text-status-error-fg font-bold' : 'text-text-dim'}`}>
              <Thermometer className="h-3 w-3" />{gpu.temp_c.toFixed(0)}°C
            </span>
          )}
          {gpu.power_w != null && (
            <span className="flex items-center gap-0.5 text-text-dim tabular-nums">
              <Zap className="h-3 w-3 text-accent-power" />{gpu.power_w.toFixed(0)}W
            </span>
          )}
          {gpu.vram_total_mb != null && (
            <span className="flex items-center gap-0.5 text-muted-foreground tabular-nums">
              <MemoryStick className="h-3 w-3" />{fmtMb(gpu.vram_used_mb ?? 0)}/{fmtMb(gpu.vram_total_mb)}
            </span>
          )}
          {gpu.busy_pct != null && (
            <span className="text-muted-foreground tabular-nums">{gpu.busy_pct.toFixed(0)}%</span>
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
  const { t } = useTranslation()
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
            <BarChart2 className="h-4 w-4 text-accent-gpu" />
            {server.name}
            <span className="text-muted-foreground font-normal text-sm">— {t('backends.clickhouseHistory')}</span>
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
            {t('common.sync')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}
        {isError && (
          <p className="text-sm text-destructive py-2">{t('backends.checkOtel')}</p>
        )}
        {data && data.length === 0 && (
          <p className="text-sm text-muted-foreground py-6 text-center">
            {t('backends.noClickhouseData', { hours })}
            <br />
            <span className="text-xs opacity-60">{t('backends.checkOtel')}</span>
          </p>
        )}

        {data && data.length > 0 && (
          <div className="space-y-5">
            <div>
              <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('backends.memUsedPct')}</p>
              <ResponsiveContainer width="100%" height={110}>
                <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                  <XAxis dataKey="ts" tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} interval="preserveStartEnd" />
                  <YAxis domain={[0, 100]} tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} unit="%" />
                  <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}%`, 'Mem Used']} />
                  <Line type="monotone" dataKey="memUsedPct" stroke="var(--theme-status-info)" dot={false} strokeWidth={2} />
                </LineChart>
              </ResponsiveContainer>
            </div>
            {hasGpu && (
              <>
                <div>
                  <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('backends.gpuTempC')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} interval="preserveStartEnd" />
                      <YAxis tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} unit="°C" />
                      <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}°C`, 'GPU Temp']} />
                      <Line type="monotone" dataKey="gpuTemp" stroke="var(--theme-status-error)" dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
                <div>
                  <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">{t('backends.gpuPowerW')}</p>
                  <ResponsiveContainer width="100%" height={110}>
                    <LineChart data={chartData} margin={{ top: 4, right: 8, bottom: 0, left: -20 }}>
                      <XAxis dataKey="ts" tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} interval="preserveStartEnd" />
                      <YAxis tick={{ fontSize: 10, fill: 'var(--theme-text-faint)' }} unit="W" />
                      <Tooltip contentStyle={tooltipStyle} formatter={(v: number) => [`${v}W`, 'GPU Power']} />
                      <Line type="monotone" dataKey="gpuPower" stroke="var(--theme-accent-power)" dot={false} strokeWidth={2} connectNulls />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ── Register GPU Server modal ──────────────────────────────────────────────────

function RegisterServerModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
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
          <DialogTitle>{t('backends.servers.registerTitle')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="server-name">{t('backends.servers.name')} <span className="text-destructive">*</span></Label>
            <Input id="server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder="e.g. gpu-node-1" />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="server-ne-url">
              {t('backends.servers.nodeExporterUrl')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span>
            </Label>
            <Input id="server-ne-url" type="url" value={nodeExporterUrl}
              onChange={(e) => setNodeExporterUrl(e.target.value)}
              placeholder={t('backends.servers.nodeExporterUrlPlaceholder')} />
            <p className="text-xs text-muted-foreground">{t('backends.servers.nodeExporterHint')}</p>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? `${t('common.register')}…` : t('common.register')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Edit GPU server modal ──────────────────────────────────────────────────────

function EditServerModal({ server, onClose }: { server: GpuServer; onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(server.name)
  const [nodeExporterUrl, setNodeExporterUrl] = useState(server.node_exporter_url ?? '')
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: UpdateGpuServerRequest = {
        name: name.trim() || undefined,
        node_exporter_url: nodeExporterUrl.trim(),
      }
      return api.updateServer(server.id, body)
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['servers'] }); onClose() },
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Pencil className="h-4 w-4 text-primary" />
            {t('backends.servers.editTitle')}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-server-name">{t('backends.servers.name')} <span className="text-destructive">*</span></Label>
            <Input id="edit-server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder="e.g. gpu-node-1" />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="edit-server-ne-url">
              {t('backends.servers.nodeExporterUrl')} <span className="text-muted-foreground font-normal">— {t('backends.servers.nodeExporterOptional')}</span>
            </Label>
            <Input id="edit-server-ne-url" type="url" value={nodeExporterUrl}
              onChange={(e) => setNodeExporterUrl(e.target.value)}
              placeholder={t('backends.servers.nodeExporterUrlPlaceholder')} />
            <p className="text-xs text-muted-foreground">{t('backends.servers.nodeExporterHint')}</p>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? `${t('common.save')}…` : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Servers table ──────────────────────────────────────────────────────────────

const PAGE_SIZE = 10

function ServersTable({
  servers,
  isLoading,
  onRegister,
  onEdit,
  onHistory,
  onDelete,
  deleteIsPending,
}: {
  servers: GpuServer[] | undefined
  isLoading: boolean
  onRegister: () => void
  onEdit: (s: GpuServer) => void
  onHistory: (s: GpuServer) => void
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const [page, setPage] = useState(1)
  const allServers = servers ?? []
  const configuredCount = allServers.filter((s) => !!s.node_exporter_url).length
  const totalPages = Math.max(1, Math.ceil(allServers.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages)
  const pageStart = (safePage - 1) * PAGE_SIZE
  const pageItems = allServers.slice(pageStart, pageStart + PAGE_SIZE)

  return (
    <div className="space-y-4">
      {/* ── Status pills + Register button ─────────────────────────── */}
      <div className="flex items-center justify-between gap-3 flex-wrap">
        {servers ? (
          <div className="flex items-center gap-2 flex-wrap">
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
              <HardDrive className="h-3 w-3 shrink-0" />
              <span className="tabular-nums">{servers.length}</span>
              <span>{t('backends.servers.registered')}</span>
            </div>
            {configuredCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-success/10 border border-status-success/30 text-xs font-medium text-status-success-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
                <span className="tabular-nums">{configuredCount}</span>
                <span>{t('backends.servers.withMetrics')}</span>
              </div>
            )}
            {servers.length - configuredCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/40 border border-border/60 text-xs font-medium text-muted-foreground/70">
                <span className="tabular-nums">{servers.length - configuredCount}</span>
                <span>{t('backends.servers.noExporter')}</span>
              </div>
            )}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
        )}

        <Button onClick={onRegister} className="shrink-0">
          <Plus className="h-4 w-4 mr-2" />{t('backends.servers.registerServer')}
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-24 items-center justify-center text-muted-foreground text-sm animate-pulse">
          {t('backends.servers.loadingServers')}
        </div>
      )}

      {allServers.length === 0 && !isLoading && (
        <Card className="border-dashed">
          <CardContent className="p-8 text-center text-muted-foreground">
            <Server className="h-8 w-8 mx-auto mb-3 opacity-25" />
            <p className="font-medium">{t('backends.servers.noServers')}</p>
            <p className="text-sm mt-1">{t('backends.servers.noServersHint')}</p>
          </CardContent>
        </Card>
      )}

      {allServers.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table className="min-w-[700px]">
              <TableHeader>
                <TableRow className="border-b border-border hover:bg-transparent">
                  <TableHead className="w-48 text-muted-foreground font-semibold">{t('backends.servers.name')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold">{t('backends.servers.nodeExporterUrl')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold min-w-64">{t('backends.servers.liveMetrics')}</TableHead>
                  <TableHead className="text-muted-foreground font-semibold w-32">{t('backends.servers.registeredAt')}</TableHead>
                  <TableHead className="text-right text-muted-foreground font-semibold w-24">{t('keys.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {pageItems.map((s) => (
                  <TableRow key={s.id} className="align-top">
                    <TableCell className="pt-4 pb-4 font-semibold text-text-bright">{s.name}</TableCell>
                    <TableCell className="pt-4 pb-4">
                      {s.node_exporter_url
                        ? <span className="font-mono text-xs text-text-dim bg-surface-code px-2 py-1 rounded">{s.node_exporter_url}</span>
                        : <span className="text-xs text-text-faint italic">{t('backends.servers.notConfigured')}</span>
                      }
                    </TableCell>
                    <TableCell className="pt-4 pb-4">
                      {s.node_exporter_url
                        ? <ServerMetricsCell serverId={s.id} />
                        : <span className="text-xs text-text-faint italic">—</span>
                      }
                    </TableCell>
                    <TableCell className="pt-4 pb-4 text-muted-foreground text-xs whitespace-nowrap">
                      {fmtDate(s.registered_at)}
                    </TableCell>
                    <TableCell className="pt-3 pb-4 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button variant="ghost" size="icon"
                          className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                          onClick={() => onHistory(s)} title={t('backends.servers.history')}>
                          <BarChart2 className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon"
                          className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10"
                          onClick={() => onEdit(s)} title={t('backends.editBackend')}>
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon"
                          className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                          onClick={() => onDelete(s.id, s.name)}
                          disabled={deleteIsPending} title={t('backends.removeBackend')}>
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {totalPages > 1 && (
              <div className="flex items-center justify-between px-4 py-2 border-t border-border">
                <span className="text-xs text-muted-foreground">
                  {pageStart + 1}–{Math.min(pageStart + PAGE_SIZE, allServers.length)} / {allServers.length}
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
    </div>
  )
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function ServersPage() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const [showRegister, setShowRegister] = useState(false)
  const [editingServer, setEditingServer] = useState<GpuServer | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)

  const { data: servers, isLoading } = useQuery({
    queryKey: ['servers'],
    queryFn: () => api.servers(),
    refetchInterval: 30_000,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteServer(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['servers'] }),
  })

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('backends.servers.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('backends.servers.description')}</p>
      </div>

      <ServersTable
        servers={servers}
        isLoading={isLoading}
        onRegister={() => setShowRegister(true)}
        onEdit={(s) => setEditingServer(s)}
        onHistory={(s) => setHistoryServer(s)}
        onDelete={(id, name) => {
          if (confirm(t('backends.deleteServerConfirm', { name }))) deleteMutation.mutate(id)
        }}
        deleteIsPending={deleteMutation.isPending}
      />

      {showRegister && <RegisterServerModal onClose={() => setShowRegister(false)} />}
      {editingServer && <EditServerModal server={editingServer} onClose={() => setEditingServer(null)} />}
      {historyServer && <ServerHistoryModal server={historyServer} onClose={() => setHistoryServer(null)} />}
    </div>
  )
}
