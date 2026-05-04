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
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle>{t('providers.servers.registerTitle')}</DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="server-name">{t('providers.servers.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={t('providers.servers.namePlaceholder')} />
          </div>

          <div className="vds-space-y-1.5">
            <Label htmlFor="server-ne-url">
              {t('providers.servers.nodeExporterUrl')} <span className="vds-text-destructive">*</span>
            </Label>
            <div className="vds-flex vds-gap-2">
              <Input
                id="server-ne-url"
                type="url"
                value={nodeExporterUrl}
                onChange={(e) => handleUrlChange(e.target.value)}
                placeholder={t('providers.servers.nodeExporterUrlPlaceholder')}
                className={verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : ''}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="vds-flex-shrink-0"
                disabled={!canVerify}
                onClick={() => verify(nodeExporterUrl.trim())}
              >
                {verifyState === 'checking'
                  ? t('providers.servers.verifying')
                  : t('providers.servers.verifyConnection')}
              </Button>
            </div>
            {verifyState === 'ok' && (
              <p className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-success">
                <CheckCircle2 className="vds-h-3.5 vds-w-3.5" />
                {t('providers.servers.connected')}
              </p>
            )}
            {verifyState === 'error' && (
              <p className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-destructive">
                <XCircle className="vds-h-3.5 vds-w-3.5" />
                {verifyError}
              </p>
            )}
            {verifyState === 'idle' && (
              <p className="vds-text-xs vds-text-dim">{t('providers.servers.nodeExporterHint')}</p>
            )}
          </div>
        </div>

        {registerMutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {registerMutation.error instanceof Error ? registerMutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
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
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <Pencil className="vds-h-4 vds-w-4 vds-text-primary" />
            {t('providers.servers.editTitle')}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-server-name">{t('providers.servers.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="edit-server-name" value={name} onChange={(e) => setName(e.target.value)}
              placeholder={t('providers.servers.namePlaceholder')} />
          </div>

          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-server-ne-url">
              {t('providers.servers.nodeExporterUrl')} <span className="vds-text-dim vds-font-400">— {t('providers.servers.nodeExporterOptional')}</span>
            </Label>
            <div className="vds-flex vds-gap-2">
              <Input id="edit-server-ne-url" type="url" value={nodeExporterUrl}
                onChange={(e) => handleUrlChange(e.target.value)}
                placeholder={t('providers.servers.nodeExporterUrlPlaceholder')}
                className={verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : ''} />
              {urlChanged && (
                <Button type="button" variant="outline" size="sm" className="vds-flex-shrink-0"
                  disabled={!canVerify}
                  onClick={() => verify(nodeExporterUrl.trim())}>
                  {verifyState === 'checking' ? t('providers.servers.verifying')
                    : verifyState === 'ok' ? <><CheckCircle2 className="vds-h-3.5 vds-w-3.5 vds-mr-1 vds-text-success" />{t('providers.servers.connected')}</>
                    : t('providers.servers.verifyConnection')}
                </Button>
              )}
            </div>
            {verifyState === 'error' && <p className="vds-text-xs vds-text-destructive vds-flex vds-items-center vds-gap-1"><XCircle className="vds-h-3 vds-w-3" />{verifyError}</p>}
            {urlChanged && verifyState === 'idle' && <p className="vds-text-xs vds-text-dim">{t('providers.servers.verifyFirst')}</p>}
            <p className="vds-text-xs vds-text-dim">{t('providers.servers.nodeExporterHint')}</p>
          </div>
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
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
    <div className="vds-space-y-4">
      {/* ── Status pills + Register button ─────────────────────────── */}
      <div className="vds-flex vds-items-center vds-justify-between vds-gap-3 vds-flex-wrap">
        {servers ? (
          <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap">
            <StatusPill icon={<HardDrive className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={servers.length} label={t('providers.servers.registered')} />
            {configuredCount > 0 && (
              <StatusPill
                icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-flex-shrink-0" />}
                count={configuredCount} label={t('providers.servers.withMetrics')}
                className="vds-bg-success/10 vds-border-1 vds-border-success/30 vds-text-success"
              />
            )}
            {servers.length - configuredCount > 0 && (
              <StatusPill
                count={servers.length - configuredCount} label={t('providers.servers.noExporter')}
                className="vds-bg-muted/40 vds-border-1 vds-border-subtle/60 vds-text-dim/70"
              />
            )}
          </div>
        ) : (
          <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>
        )}

        <Button onClick={onRegister} className="vds-flex-shrink-0">
          <Plus className="vds-h-4 vds-w-4 vds-mr-2" />{t('providers.servers.registerServer')}
        </Button>
      </div>

      {isLoading && (
        <div
          aria-busy="true"
          aria-label={t('providers.servers.loadingServers')}
          className="vds-flex vds-h-24 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse"
        >
          {t('providers.servers.loadingServers')}
        </div>
      )}

      {allServers.length === 0 && !isLoading && (
        <Card className="vds-border-dashed">
          <CardContent className="vds-p-8 vds-text-center vds-text-dim">
            <Server className="vds-h-8 vds-w-8 vds-mx-auto vds-mb-3 vds-opacity-25" />
            <p className="vds-font-500">{t('providers.servers.noServers')}</p>
            <p className="vds-text-sm vds-mt-1">{t('providers.servers.noServersHint')}</p>
          </CardContent>
        </Card>
      )}

      {allServers.length > 0 && (
        <DataTable
          minWidth="700px"
          footer={totalPages > 1 ? (
            <div className="vds-flex vds-items-center vds-justify-between vds-px-6 vds-py-2">
              <span className="vds-text-xs vds-text-dim">
                {pageStart + 1}–{Math.min(pageStart + PAGE_SIZE, allServers.length)} / {allServers.length}
              </span>
              <div className="vds-flex vds-items-center vds-gap-1">
                <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                  aria-label={t('common.prevPage')}
                  onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={safePage <= 1}>
                  <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
                </Button>
                <span className="vds-text-xs vds-text-dim vds-px-1">{safePage} / {totalPages}</span>
                <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                  aria-label={t('common.nextPage')}
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={safePage >= totalPages}>
                  <ChevronRight className="vds-h-3.5 vds-w-3.5" />
                </Button>
              </div>
            </div>
          ) : undefined}
        >
          <TableHeader>
            <TableRow className="vds-hover:bg-transparent">
              <TableHead className="vds-w-48 vds-whitespace-nowrap">{t('providers.servers.name')}</TableHead>
              <TableHead className="vds-whitespace-nowrap">{t('providers.servers.nodeExporterUrl')}</TableHead>
              <TableHead className="vds-min-w-64 vds-whitespace-nowrap">{t('providers.servers.liveMetrics')}</TableHead>
              <TableHead className="vds-w-32 vds-whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="vds-text-right vds-w-24 vds-whitespace-nowrap">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {pageItems.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="vds-font-600 vds-text-bright">{s.name}</TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <span className="vds-font-mono vds-text-xs vds-text-dim vds-bg-surface-code vds-px-2 vds-py-1 vds-rounded">{s.node_exporter_url}</span>
                    : <span className="vds-text-xs vds-text-faint vds-italic">{t('providers.servers.notConfigured')}</span>
                  }
                </TableCell>
                <TableCell>
                  {s.node_exporter_url
                    ? <ServerMetricsCell serverId={s.id} />
                    : <span className="vds-text-xs vds-text-faint vds-italic">—</span>
                  }
                </TableCell>
                <TableCell className="vds-text-dim vds-text-xs vds-whitespace-nowrap">
                  {fmtDateOnly(s.registered_at, tz)}
                </TableCell>
                <TableCell className="vds-text-right">
                  <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
                    <Button variant="ghost" size="icon"
                      className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-accent-gpu vds-hover:bg-hover-gpu/10"
                      aria-label={t('providers.servers.history')}
                      onClick={() => onHistory(s)} title={t('providers.servers.history')}>
                      <BarChart2 className="vds-h-4 vds-w-4" />
                    </Button>
                    <Button variant="ghost" size="icon"
                      className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-primary vds-hover:bg-primary/10"
                      aria-label={t('providers.editProvider')}
                      onClick={() => onEdit(s)} title={t('providers.editProvider')}>
                      <Pencil className="vds-h-4 vds-w-4" />
                    </Button>
                    <Button variant="ghost" size="icon"
                      className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-error vds-hover:bg-error/10"
                      aria-label={t('providers.removeProvider')}
                      onClick={() => onDelete(s.id, s.name)}
                      disabled={deleteIsPending} title={t('providers.removeProvider')}>
                      <Trash2 className="vds-h-4 vds-w-4" />
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
  usePageGuard('provider_manage')
  const { t } = useTranslation()

  const [showRegister, setShowRegister] = useState(false)
  const [editingServer, setEditingServer] = useState<GpuServer | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null)

  const { data: serversData, isLoading } = useQuery(serversQuery())
  const servers = serversData?.servers

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteServer(id),
    { invalidateKey: ['servers'], onSuccess: () => setDeleteTarget(null) },
  )

  const handleRegister = useCallback(() => setShowRegister(true), [])
  const handleEdit = useCallback((s: GpuServer) => setEditingServer(s), [])
  const handleHistory = useCallback((s: GpuServer) => setHistoryServer(s), [])
  const handleDelete = useCallback((id: string, name: string) => {
    setDeleteTarget({ id, name })
  }, [])

  return (
    <div className="vds-space-y-6">
      <div>
        <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('providers.servers.title')}</h1>
        <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('providers.servers.description')}</p>
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
