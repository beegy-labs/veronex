'use client'

import { useState, useMemo, useEffect, useCallback } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import type { GpuServer } from '@/lib/types'
import { Plus, Trash2, RefreshCw, Server, ListFilter, Pencil, BarChart2, ChevronLeft, ChevronRight, Search } from 'lucide-react'
import { ServerMetricsCompact } from '@/components/server-metrics-cell'
import { fmtMb } from '@/lib/chart-theme'
import { ServerHistoryModal } from '@/components/server-history-modal'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Card, CardContent } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
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
import { providersQuery } from '@/lib/queries'
import { extractHost, StatusBadge } from './shared'
import { OllamaProviderModelsModal } from './modals'
import type { Provider } from '@/lib/types'
import { PAGE_SIZE, OllamaSyncSection, OllamaCapacitySection } from './ollama-sections'
import { OllamaLabSection } from './ollama-lab-section'

// ── Tab: Ollama providers ───────────────────────────────────────────────────────

export function OllamaTab({
  servers,
  onRegister,
  onEdit,
  onSync,
  syncPending,
  syncVars,
  onDelete,
  deleteIsPending,
}: {
  servers: GpuServer[]
  onRegister: () => void
  onEdit: (b: Provider) => void
  onSync: (id: string) => void
  syncPending: boolean
  syncVars: string | undefined
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()

  // Persist active sub-tab via URL hash
  const [activeTab, setActiveTab] = useState<'providers' | 'capacity' | 'lab'>(() => {
    if (typeof window !== 'undefined') {
      const hash = window.location.hash.slice(1)
      if (hash === 'capacity') return 'capacity'
      if (hash === 'lab') return 'lab'
    }
    return 'providers'
  })
  const handleTabChange = useCallback((v: string) => {
    const tab = v as 'providers' | 'capacity' | 'lab'
    setActiveTab(tab)
    window.history.replaceState(null, '', tab === 'providers' ? window.location.pathname + window.location.search : `#${tab}`)
  }, [])

  return (
    <div className="vds-space-y-4">
      <Tabs value={activeTab} onValueChange={handleTabChange}>
        <TabsList>
          <TabsTrigger value="providers">{t('nav.ollama')}</TabsTrigger>
          <TabsTrigger value="capacity">{t('nav.capacity')}</TabsTrigger>
          <TabsTrigger value="lab">{t('providers.ollama.labTitle')}</TabsTrigger>
        </TabsList>

        {/* ── 프로바이더 목록 탭 ────────────────────────────────────────────────── */}
        <TabsContent value="providers" className="vds-mt-6">
          <ProvidersListTab
            servers={servers}
            onRegister={onRegister}
            onEdit={onEdit}
            onSync={onSync}
            syncPending={syncPending}
            syncVars={syncVars}
            onDelete={onDelete}
            deleteIsPending={deleteIsPending}
          />
        </TabsContent>

        {/* ── 동시성 제어 탭 ────────────────────────────────────────────────────── */}
        <TabsContent value="capacity" className="vds-mt-6">
          <OllamaCapacitySection />
        </TabsContent>

        {/* ── Ollama Lab 탭 ────────────────────────────────────────────────────── */}
        <TabsContent value="lab" className="vds-mt-6">
          <OllamaLabSection />
        </TabsContent>
      </Tabs>
    </div>
  )
}

// ── 프로바이더 목록 (기존 내용) ─────────────────────────────────────────────────

function ProvidersListTab({
  servers,
  onRegister,
  onEdit,
  onSync,
  syncPending,
  syncVars,
  onDelete,
  deleteIsPending,
}: {
  servers: GpuServer[]
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
  const serverMap = useMemo(() => new Map(servers.map((s) => [s.id, s])), [servers])

  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [page, setPage] = useState(1)
  const [viewModelsProvider, setViewModelsProvider] = useState<Provider | null>(null)
  const [historyServer, setHistoryServer] = useState<GpuServer | null>(null)

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300)
    return () => clearTimeout(timer)
  }, [search])

  useEffect(() => { setPage(1) }, [debouncedSearch])

  const { data, isLoading, error } = useQuery(
    providersQuery({ provider_type: 'ollama', search: debouncedSearch || undefined, page, limit: PAGE_SIZE })
  )

  const providers = data?.providers ?? []
  const total = data?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))

  return (
    <div className="vds-space-y-4">
      <div className="vds-flex vds-items-center vds-justify-between vds-gap-3 vds-flex-wrap">
        <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap">
          {isLoading ? (
            <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('providers.ollama.loadingProviders')}</p>
          ) : (
            <span className="vds-text-sm vds-text-dim">
              {t('providers.servers.registered')}: <span className="vds-font-500 vds-text-primary">{total}</span>
            </span>
          )}
        </div>

        <div className="vds-flex vds-items-center vds-gap-2">
          <div className="vds-relative">
            <Search className="vds-absolute vds-left-2.5 vds-top-1/2 -translate-y-1/2 vds-h-3.5 vds-w-3.5 vds-text-dim vds-pointer-events-none" />
            <Input
              className="vds-h-8 vds-text-sm vds-w-48 vds-pl-8"
              placeholder={t('providers.ollama.searchProvider')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <Button onClick={onRegister} className="vds-flex-shrink-0">
            <Plus className="vds-h-4 vds-w-4 vds-mr-2" />{t('providers.ollama.registerProvider')}
          </Button>
        </div>
      </div>

      {error && (
        <Card className="vds-border-destructive/40 vds-bg-destructive/5">
          <CardContent className="vds-p-5 vds-text-destructive">
            <p className="vds-font-600">{t('providers.ollama.failedProviders')}</p>
            <p className="vds-text-sm vds-mt-1 vds-opacity-75">
              {error instanceof Error ? error.message : t('common.unknownError')}
            </p>
          </CardContent>
        </Card>
      )}

      {!isLoading && providers.length === 0 && !error && (
        <Card className="vds-border-dashed">
          <CardContent className="vds-p-10 vds-text-center vds-text-dim">
            <Server className="vds-h-10 vds-w-10 vds-mx-auto vds-mb-3 vds-opacity-25" />
            <p className="vds-font-500">{t('providers.ollama.noBackends')}</p>
            <p className="vds-text-sm vds-mt-1">{t('providers.ollama.noBackendsHint')}</p>
          </CardContent>
        </Card>
      )}

      {providers.length > 0 && (
        <DataTable
          minWidth="800px"
          footer={totalPages > 1 ? (
            <div className="vds-flex vds-items-center vds-justify-between vds-px-6 vds-py-2">
              <span className="vds-text-xs vds-text-dim">
                {(page - 1) * PAGE_SIZE + 1}–{Math.min(page * PAGE_SIZE, total)} / {total}
              </span>
              <div className="vds-flex vds-items-center vds-gap-1">
                <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                  aria-label={t('common.prevPage')}
                  onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={page <= 1}>
                  <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
                </Button>
                <span className="vds-text-xs vds-text-dim vds-px-1">{page} / {totalPages}</span>
                <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                  aria-label={t('common.nextPage')}
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={page >= totalPages}>
                  <ChevronRight className="vds-h-3.5 vds-w-3.5" />
                </Button>
              </div>
            </div>
          ) : undefined}
        >
          <TableHeader>
            <TableRow className="vds-hover:bg-transparent">
              <TableHead>{t('providers.ollama.name')}</TableHead>
              <TableHead>{t('providers.ollama.server')}</TableHead>
              <TableHead className="vds-min-w-52">{t('providers.servers.liveMetrics')}</TableHead>
              <TableHead className="vds-whitespace-nowrap">{t('providers.ollama.status')}</TableHead>
              <TableHead className="vds-whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="vds-text-right vds-whitespace-nowrap">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {providers.map((b) => {
              const linkedServer = b.server_id ? serverMap.get(b.server_id) : null
              return (
                <TableRow key={b.id}>
                  <TableCell>
                    <div className="vds-flex vds-items-center vds-gap-2 vds-mb-1">
                      <span className="vds-font-600 vds-text-bright">{b.name}</span>
                      {b.is_free_tier && (
                        <Badge variant="outline" className="vds-bg-warning/15 vds-text-warning vds-border-warning/30 vds-text-[10px] vds-px-2 vds-py-0.5">
                          {t('providers.ollama.freeTier')}
                        </Badge>
                      )}
                    </div>
                    {b.url && (
                      <span className="vds-font-mono vds-text-xs vds-text-dim/70">{extractHost(b.url)}</span>
                    )}
                  </TableCell>

                  <TableCell>
                    <div className="vds-space-y-1 vds-text-xs">
                      {linkedServer ? (
                        <div className="vds-flex vds-items-center vds-gap-1.5 vds-text-dim">
                          <Server className="vds-h-3 vds-w-3 vds-text-dim/70 vds-flex-shrink-0" />
                          <span className="vds-font-500">{linkedServer.name}</span>
                        </div>
                      ) : (
                        <span className="vds-text-faint vds-italic vds-text-xs">{t('providers.ollama.noServerLinked')}</span>
                      )}
                      <div className="vds-flex vds-items-center vds-gap-3 vds-text-dim vds-pl-0.5">
                        {b.gpu_index !== null && (
                          <span className="vds-flex vds-items-center vds-gap-1">
                            <span className="vds-text-[10px] vds-font-600 vds-text-dim/70 vds-uppercase">{t('providers.ollama.gpuLabel')}</span>
                            <span className="vds-tabular-nums vds-font-mono">{b.gpu_index}</span>
                          </span>
                        )}
                        {b.total_vram_mb > 0 && (
                          <span className="vds-flex vds-items-center vds-gap-1">
                            <span className="vds-text-[10px] vds-font-600 vds-text-dim/70 vds-uppercase">{t('providers.ollama.vram')}</span>
                            <span className="vds-tabular-nums vds-font-mono">{fmtMb(b.total_vram_mb)}</span>
                          </span>
                        )}
                        {b.gpu_index === null && b.total_vram_mb === 0 && linkedServer && (
                          <span className="vds-text-faint vds-italic">{t('providers.servers.notConfigured')}</span>
                        )}
                      </div>
                    </div>
                  </TableCell>

                  <TableCell>
                    {linkedServer
                      ? <ServerMetricsCompact serverId={linkedServer.id} gpuIndex={b.gpu_index} />
                      : <span className="vds-text-xs vds-text-faint vds-italic">—</span>
                    }
                  </TableCell>

                  <TableCell>
                    <StatusBadge status={b.status} />
                  </TableCell>

                  <TableCell className="vds-text-xs vds-text-dim vds-whitespace-nowrap">
                    {fmtDateOnly(b.registered_at, tz)}
                  </TableCell>

                  <TableCell className="vds-text-right">
                    <TooltipProvider delayDuration={200}>
                      <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
                        {linkedServer && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <Button variant="ghost" size="icon"
                                className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-accent-gpu vds-hover:bg-hover-gpu/10"
                                aria-label={t('providers.servers.history')}
                                onClick={() => setHistoryServer(linkedServer)}>
                                <BarChart2 className="vds-h-4 vds-w-4" />
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>{t('providers.servers.history')}</TooltipContent>
                          </Tooltip>
                        )}
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-primary"
                              aria-label={t('common.sync')}
                              onClick={() => onSync(b.id)}
                              disabled={syncPending && syncVars === b.id}>
                              <RefreshCw className={
                                syncPending && syncVars === b.id
                                  ? 'vds-h-4 vds-w-4 vds-animate-spin' : 'vds-h-4 vds-w-4'
                              } />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('common.sync')}</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-accent-gpu vds-hover:bg-hover-gpu/10"
                              aria-label={t('providers.ollama.modelSelection')}
                              onClick={() => setViewModelsProvider(b)}>
                              <ListFilter className="vds-h-4 vds-w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.ollama.modelSelection')}</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-primary vds-hover:bg-primary/10"
                              aria-label={t('providers.ollama.editTitle')}
                              onClick={() => onEdit(b)}>
                              <Pencil className="vds-h-4 vds-w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.ollama.editTitle')}</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-error vds-hover:bg-error/10"
                              aria-label={t('providers.removeProvider')}
                              onClick={() => onDelete(b.id, b.name)}
                              disabled={deleteIsPending}>
                              <Trash2 className="vds-h-4 vds-w-4" />
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

      {viewModelsProvider && (
        <OllamaProviderModelsModal
          provider={viewModelsProvider}
          onClose={() => setViewModelsProvider(null)}
        />
      )}

      {historyServer && (
        <ServerHistoryModal
          server={historyServer}
          onClose={() => setHistoryServer(null)}
        />
      )}
    </div>
  )
}
