'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { GpuServer, RegisterGpuServerRequest, UpdateGpuServerRequest } from '@/lib/types'
import {
  Plus, Trash2, BarChart2, Pencil,
  Server, HardDrive,
  ChevronLeft, ChevronRight,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent } from '@/components/ui/card'
import { ServerMetricsCell } from '@/components/server-metrics-cell'
import { ServerHistoryModal } from '@/components/server-history-modal'
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
import { useTranslation } from '@/i18n'

// ── Helpers ────────────────────────────────────────────────────────────────────

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

// ── Live metrics cell ──────────────────────────────────────────────────────────

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
        <DataTable
          minWidth="700px"
          footer={totalPages > 1 ? (
            <div className="flex items-center justify-between px-6 py-2">
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
          ) : undefined}
        >
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="w-48">{t('backends.servers.name')}</TableHead>
              <TableHead>{t('backends.servers.nodeExporterUrl')}</TableHead>
              <TableHead className="min-w-64">{t('backends.servers.liveMetrics')}</TableHead>
              <TableHead className="w-32">{t('backends.servers.registeredAt')}</TableHead>
              <TableHead className="text-right w-24">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {pageItems.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="font-semibold text-text-bright">{s.name}</TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <span className="font-mono text-xs text-text-dim bg-surface-code px-2 py-1 rounded">{s.node_exporter_url}</span>
                    : <span className="text-xs text-text-faint italic">{t('backends.servers.notConfigured')}</span>
                  }
                </TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <ServerMetricsCell serverId={s.id} />
                    : <span className="text-xs text-text-faint italic">—</span>
                  }
                </TableCell>
                <TableCell className="text-muted-foreground text-xs whitespace-nowrap">
                  {fmtDate(s.registered_at)}
                </TableCell>
                <TableCell className="text-right">
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
        </DataTable>
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
