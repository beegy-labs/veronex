'use client'

import { useState, useMemo, useCallback, memo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { serversQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { GpuServer, RegisterGpuServerRequest, UpdateGpuServerRequest } from '@/lib/types'
import { useVerifyUrl } from '@/hooks/use-verify-url'
import {
  Plus, Trash2, BarChart2, Pencil,
  Server, HardDrive,
  ChevronLeft, ChevronRight,
  CheckCircle2, XCircle,
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
import { ConfirmDialog } from '@/components/confirm-dialog'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDateOnly } from '@/lib/date'
import { StatusPill } from '@/components/status-pill'

// ── Live metrics cell ──────────────────────────────────────────────────────────

// ── Register GPU Server modal ──────────────────────────────────────────────────

function RegisterServerModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [nodeExporterUrl, setNodeExporterUrl] = useState('')

  const { verifyState, verifyError, verifiedUrl, verify, handleUrlChange: onVerifyReset } = useVerifyUrl({
    verifyFn: api.verifyServer,
    labels: {
      duplicate: t('providers.servers.duplicateUrl'),
      network: t('providers.servers.networkError'),
      unreachable: t('providers.servers.unreachableError'),
      fallback: t('providers.servers.connectionFailed'),
    },
  })

  const handleUrlChange = (val: string) => { setNodeExporterUrl(val); onVerifyReset() }

  const registerMutation = useApiMutation(
    () => api.registerServer({ name: name.trim(), node_exporter_url: nodeExporterUrl.trim() }),
    { invalidateKey: ['servers'], onSuccess: () => onClose() },
  )

  const canVerify = !!nodeExporterUrl.trim() && verifyState !== 'checking'
  const isVerified = verifyState === 'ok' && nodeExporterUrl.trim() === verifiedUrl
  const canRegister = !!name.trim() && isVerified && !registerMutation.isPending

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('providers.servers.registerTitle')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="server-name">{t('providers.servers.name')} <span className="text-destructive">*</span></Label>
            <Input id="server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={t('providers.servers.namePlaceholder')} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="server-ne-url">
              {t('providers.servers.nodeExporterUrl')} <span className="text-destructive">*</span>
            </Label>
            <div className="flex gap-2">
              <Input
                id="server-ne-url"
                type="url"
                value={nodeExporterUrl}
                onChange={(e) => handleUrlChange(e.target.value)}
                placeholder={t('providers.servers.nodeExporterUrlPlaceholder')}
                className={verifyState === 'ok' ? 'border-status-success' : verifyState === 'error' ? 'border-destructive' : ''}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="shrink-0"
                disabled={!canVerify}
                onClick={() => verify(nodeExporterUrl.trim())}
              >
                {verifyState === 'checking'
                  ? t('providers.servers.verifying')
                  : t('providers.servers.verifyConnection')}
              </Button>
            </div>
            {verifyState === 'ok' && (
              <p className="flex items-center gap-1.5 text-xs text-status-success-fg">
                <CheckCircle2 className="h-3.5 w-3.5" />
                {t('providers.servers.connected')}
              </p>
            )}
            {verifyState === 'error' && (
              <p className="flex items-center gap-1.5 text-xs text-destructive">
                <XCircle className="h-3.5 w-3.5" />
                {verifyError}
              </p>
            )}
            {verifyState === 'idle' && (
              <p className="text-xs text-muted-foreground">{t('providers.servers.nodeExporterHint')}</p>
            )}
          </div>
        </div>

        {registerMutation.error && (
          <p className="text-sm text-destructive">
            {registerMutation.error instanceof Error ? registerMutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3 flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button
            onClick={() => registerMutation.mutate(undefined)}
            disabled={!canRegister}
            title={!isVerified ? t('providers.servers.verifyFirst') : undefined}
          >
            {registerMutation.isPending ? `${t('common.register')}…` : t('common.register')}
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

  const { verifyState, verifyError, verifiedUrl, verify, handleUrlChange: onVerifyReset } = useVerifyUrl({
    verifyFn: api.verifyServer,
    labels: {
      duplicate: t('providers.servers.duplicateUrl'),
      network: t('providers.servers.networkError'),
      unreachable: t('providers.servers.unreachableError'),
      fallback: t('providers.servers.connectionFailed'),
    },
    initialUrl: server.node_exporter_url ?? '',
  })

  const urlChanged = nodeExporterUrl.trim() !== (server.node_exporter_url ?? '')

  const handleUrlChange = (val: string) => { setNodeExporterUrl(val); onVerifyReset() }

  const mutation = useApiMutation(
    () => api.updateServer(server.id, { name: name.trim() || undefined, node_exporter_url: nodeExporterUrl.trim() }),
    { invalidateKey: ['servers'], onSuccess: () => onClose() },
  )

  const canVerify = !!nodeExporterUrl.trim() && verifyState !== 'checking'
  const isVerified = !urlChanged || (verifyState === 'ok' && nodeExporterUrl.trim() === verifiedUrl)
  const canSave = !!name.trim() && isVerified && !mutation.isPending

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Pencil className="h-4 w-4 text-primary" />
            {t('providers.servers.editTitle')}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-server-name">{t('providers.servers.name')} <span className="text-destructive">*</span></Label>
            <Input id="edit-server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={t('providers.servers.namePlaceholder')} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="edit-server-ne-url">
              {t('providers.servers.nodeExporterUrl')} <span className="text-muted-foreground font-normal">— {t('providers.servers.nodeExporterOptional')}</span>
            </Label>
            <div className="flex gap-2">
              <Input id="edit-server-ne-url" type="url" value={nodeExporterUrl}
                onChange={(e) => handleUrlChange(e.target.value)}
                placeholder={t('providers.servers.nodeExporterUrlPlaceholder')}
                className={verifyState === 'ok' ? 'border-status-success' : verifyState === 'error' ? 'border-destructive' : ''} />
              {urlChanged && (
                <Button type="button" variant="outline" size="sm" className="shrink-0"
                  disabled={!canVerify}
                  onClick={() => verify(nodeExporterUrl.trim())}>
                  {verifyState === 'checking' ? t('providers.servers.verifying')
                    : verifyState === 'ok' ? <><CheckCircle2 className="h-3.5 w-3.5 mr-1 text-status-success-fg" />{t('providers.servers.connected')}</>
                    : t('providers.servers.verifyConnection')}
                </Button>
              )}
            </div>
            {verifyState === 'error' && <p className="text-xs text-destructive flex items-center gap-1"><XCircle className="h-3 w-3" />{verifyError}</p>}
            {urlChanged && verifyState === 'idle' && <p className="text-xs text-muted-foreground">{t('providers.servers.verifyFirst')}</p>}
            <p className="text-xs text-muted-foreground">{t('providers.servers.nodeExporterHint')}</p>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3 flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate(undefined)} disabled={!canSave}>
            {mutation.isPending ? `${t('common.save')}…` : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Servers table ──────────────────────────────────────────────────────────────

const PAGE_SIZE = 10

interface ServersTableHandlers {
  onRegister: () => void
  onEdit: (s: GpuServer) => void
  onHistory: (s: GpuServer) => void
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}

const ServersTable = memo(function ServersTable({
  servers,
  isLoading,
  handlers,
}: {
  servers: GpuServer[] | undefined
  isLoading: boolean
  handlers: ServersTableHandlers
}) {
  const { onRegister, onEdit, onHistory, onDelete, deleteIsPending } = handlers
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [page, setPage] = useState(1)
  const allServers = servers ?? []
  const configuredCount = useMemo(() => allServers.filter((s) => !!s.node_exporter_url).length, [allServers])
  const { totalPages, safePage, pageStart, pageItems } = useMemo(() => {
    const totalPages = Math.max(1, Math.ceil(allServers.length / PAGE_SIZE))
    const safePage = Math.min(page, totalPages)
    const pageStart = (safePage - 1) * PAGE_SIZE
    const pageItems = allServers.slice(pageStart, pageStart + PAGE_SIZE)
    return { totalPages, safePage, pageStart, pageItems }
  }, [allServers, page])

  return (
    <div className="space-y-4">
      {/* ── Status pills + Register button ─────────────────────────── */}
      <div className="flex items-center justify-between gap-3 flex-wrap">
        {servers ? (
          <div className="flex items-center gap-2 flex-wrap">
            <StatusPill icon={<HardDrive className="h-3 w-3 shrink-0" />} count={servers.length} label={t('providers.servers.registered')} />
            {configuredCount > 0 && (
              <StatusPill
                icon={<span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />}
                count={configuredCount} label={t('providers.servers.withMetrics')}
                className="bg-status-success/10 border border-status-success/30 text-status-success-fg"
              />
            )}
            {servers.length - configuredCount > 0 && (
              <StatusPill
                count={servers.length - configuredCount} label={t('providers.servers.noExporter')}
                className="bg-muted/40 border border-border/60 text-muted-foreground/70"
              />
            )}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
        )}

        <Button onClick={onRegister} className="shrink-0">
          <Plus className="h-4 w-4 mr-2" />{t('providers.servers.registerServer')}
        </Button>
      </div>

      {isLoading && (
        <div
          aria-busy="true"
          aria-label={t('providers.servers.loadingServers')}
          className="flex h-24 items-center justify-center text-muted-foreground text-sm animate-pulse"
        >
          {t('providers.servers.loadingServers')}
        </div>
      )}

      {allServers.length === 0 && !isLoading && (
        <Card className="border-dashed">
          <CardContent className="p-8 text-center text-muted-foreground">
            <Server className="h-8 w-8 mx-auto mb-3 opacity-25" />
            <p className="font-medium">{t('providers.servers.noServers')}</p>
            <p className="text-sm mt-1">{t('providers.servers.noServersHint')}</p>
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
                  aria-label={t('common.prevPage')}
                  onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={safePage <= 1}>
                  <ChevronLeft className="h-3.5 w-3.5" />
                </Button>
                <span className="text-xs text-muted-foreground px-1">{safePage} / {totalPages}</span>
                <Button variant="outline" size="icon" className="h-7 w-7"
                  aria-label={t('common.nextPage')}
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={safePage >= totalPages}>
                  <ChevronRight className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          ) : undefined}
        >
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="w-48 whitespace-nowrap">{t('providers.servers.name')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('providers.servers.nodeExporterUrl')}</TableHead>
              <TableHead className="min-w-64 whitespace-nowrap">{t('providers.servers.liveMetrics')}</TableHead>
              <TableHead className="w-32 whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="text-right w-24 whitespace-nowrap">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {pageItems.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="font-semibold text-text-bright">{s.name}</TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <span className="font-mono text-xs text-text-dim bg-surface-code px-2 py-1 rounded">{s.node_exporter_url}</span>
                    : <span className="text-xs text-text-faint italic">{t('providers.servers.notConfigured')}</span>
                  }
                </TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <ServerMetricsCell serverId={s.id} />
                    : <span className="text-xs text-text-faint italic">—</span>
                  }
                </TableCell>
                <TableCell className="text-muted-foreground text-xs whitespace-nowrap">
                  {fmtDateOnly(s.registered_at, tz)}
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex items-center justify-end gap-1">
                    <Button variant="ghost" size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                      aria-label={t('providers.servers.history')}
                      onClick={() => onHistory(s)} title={t('providers.servers.history')}>
                      <BarChart2 className="h-4 w-4" />
                    </Button>
                    <Button variant="ghost" size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10"
                      aria-label={t('providers.editProvider')}
                      onClick={() => onEdit(s)} title={t('providers.editProvider')}>
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button variant="ghost" size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                      aria-label={t('providers.removeProvider')}
                      onClick={() => onDelete(s.id, s.name)}
                      disabled={deleteIsPending} title={t('providers.removeProvider')}>
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
})

// ── Page ──────────────────────────────────────────────────────────────────────

export default function ServersPage() {
  usePageGuard('servers')
  const { t } = useTranslation()

  const [showRegister, setShowRegister] = useState(false)
  const [editingServer, setEditingServer] = useState<GpuServer | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null)

  const { data: serversData, isLoading } = useQuery(serversQuery())
  const servers = serversData?.servers

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteServer(id),
    { invalidateKey: ['servers'] },
  )

  const handleRegister = useCallback(() => setShowRegister(true), [])
  const handleEdit = useCallback((s: GpuServer) => setEditingServer(s), [])
  const handleHistory = useCallback((s: GpuServer) => setHistoryServer(s), [])
  const handleDelete = useCallback((id: string, name: string) => {
    setDeleteTarget({ id, name })
  }, [])

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('providers.servers.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('providers.servers.description')}</p>
      </div>

      <ServersTable
        servers={servers}
        isLoading={isLoading}
        handlers={{
          onRegister: handleRegister,
          onEdit: handleEdit,
          onHistory: handleHistory,
          onDelete: handleDelete,
          deleteIsPending: deleteMutation.isPending,
        }}
      />

      {showRegister && <RegisterServerModal onClose={() => setShowRegister(false)} />}
      {editingServer && <EditServerModal server={editingServer} onClose={() => setEditingServer(null)} />}
      {historyServer && <ServerHistoryModal server={historyServer} onClose={() => setHistoryServer(null)} />}
      {deleteTarget && (
        <ConfirmDialog
          open
          title={t('providers.removeProvider')}
          description={t('providers.deleteServerConfirm', { name: deleteTarget.name })}
          confirmLabel={deleteMutation.isPending ? t('common.deleting') : t('common.delete')}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isLoading={deleteMutation.isPending}
        />
      )}
    </div>
  )
}
