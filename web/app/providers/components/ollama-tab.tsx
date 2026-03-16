'use client'

import { useState, useMemo } from 'react'
import type { Provider, GpuServer } from '@/lib/types'
import { Plus, Trash2, RefreshCw, Server, ListFilter, Pencil, BarChart2, ChevronLeft, ChevronRight } from 'lucide-react'
import { ServerMetricsCompact } from '@/components/server-metrics-cell'
import { fmtMb } from '@/lib/chart-theme'
import { ServerHistoryModal } from '@/components/server-history-modal'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDateOnly } from '@/lib/date'
import { getOllamaProviders, countByStatus } from '@/lib/utils'
import { extractHost, StatusBadge, StatusPill } from './shared'
import { OllamaProviderModelsModal } from './modals'
import { PAGE_SIZE, OllamaSyncSection, OllamaCapacitySection } from './ollama-sections'

// ── Tab: Ollama providers ───────────────────────────────────────────────────────

export function OllamaTab({
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
  const ollama = useMemo(() => getOllamaProviders(providers), [providers])
  const serverMap = useMemo(() => new Map(servers.map((s) => [s.id, s])), [servers])
  const ollamaCounts = useMemo(() => countByStatus(ollama), [ollama])
  const onlineCount = ollamaCounts['online'] ?? 0
  const offlineCount = ollamaCounts['offline'] ?? 0
  const degradedCount = ollamaCounts['degraded'] ?? 0
  const [viewModelsProvider, setViewModelsProvider] = useState<Provider | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)
  const [page, setPage] = useState(1)
  const { totalPages, safePage, pageStart, pageItems } = useMemo(() => {
    const totalPages = Math.max(1, Math.ceil(ollama.length / PAGE_SIZE))
    const safePage = Math.min(page, totalPages)
    const pageStart = (safePage - 1) * PAGE_SIZE
    const pageItems = ollama.slice(pageStart, pageStart + PAGE_SIZE)
    return { totalPages, safePage, pageStart, pageItems }
  }, [ollama, page])

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        {providers ? (
          <div className="flex items-center gap-2 flex-wrap">
            <StatusPill icon={<Server className="h-3 w-3 shrink-0" />} count={ollama.length} label={t('providers.servers.registered')} />
            {onlineCount > 0 && (
              <StatusPill
                icon={<span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />}
                count={onlineCount} label={t('common.online')}
                className="bg-status-success/10 border border-status-success/30 text-status-success-fg"
              />
            )}
            {degradedCount > 0 && (
              <StatusPill
                icon={<span className="h-1.5 w-1.5 rounded-full bg-status-warning shrink-0" />}
                count={degradedCount} label={t('common.degraded')}
                className="bg-status-warning/10 border border-status-warning/30 text-status-warning-fg"
              />
            )}
            {offlineCount > 0 && (
              <StatusPill
                icon={<span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />}
                count={offlineCount} label={t('common.offline')}
                className="bg-status-error/10 border border-status-error/30 text-status-error-fg"
              />
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
          {t('providers.ollama.loadingProviders')}
        </div>
      )}

      {error && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardContent className="p-5 text-destructive">
            <p className="font-semibold">{t('providers.ollama.failedProviders')}</p>
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
              <TableHead>{t('providers.ollama.name')}</TableHead>
              <TableHead>{t('providers.ollama.server')}</TableHead>
              <TableHead className="min-w-52">{t('providers.servers.liveMetrics')}</TableHead>
              <TableHead>{t('providers.ollama.status')}</TableHead>
              <TableHead>{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="text-right">{t('keys.actions')}</TableHead>
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
                            <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-2 py-0.5">
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
                                <span className="text-[10px] font-semibold text-muted-foreground/70 uppercase">{t('providers.ollama.gpuLabel')}</span>
                                <span className="tabular-nums font-mono">{b.gpu_index}</span>
                              </span>
                            )}
                            {b.total_vram_mb > 0 && (
                              <span className="flex items-center gap-1">
                                <span className="text-[10px] font-semibold text-muted-foreground/70 uppercase">{t('providers.ollama.vram')}</span>
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
                                    aria-label={t('providers.servers.history')}
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
                                  aria-label={t('common.sync')}
                                  onClick={() => onSync(b.id)}
                                  disabled={syncPending && syncVars === b.id}>
                                  <RefreshCw className={
                                    syncPending && syncVars === b.id
                                      ? 'h-4 w-4 animate-spin' : 'h-4 w-4'
                                  } />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>{t('common.sync')}</TooltipContent>
                            </Tooltip>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button variant="ghost" size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                                  aria-label={t('providers.ollama.modelSelection')}
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
                                  aria-label={t('providers.ollama.editTitle')}
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
                                  aria-label={t('providers.removeProvider')}
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
