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
    <div className="space-y-4">
      <Tabs value={activeTab} onValueChange={handleTabChange}>
        <TabsList>
          <TabsTrigger value="providers">{t('nav.ollama')}</TabsTrigger>
          <TabsTrigger value="capacity">{t('nav.capacity')}</TabsTrigger>
          <TabsTrigger value="lab">{t('providers.ollama.labTitle')}</TabsTrigger>
        </TabsList>

        {/* ── 프로바이더 목록 탭 ────────────────────────────────────────────────── */}
        <TabsContent value="providers" className="mt-6">
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
        <TabsContent value="capacity" className="mt-6">
          <OllamaCapacitySection />
        </TabsContent>

        {/* ── Ollama Lab 탭 ────────────────────────────────────────────────────── */}
        <TabsContent value="lab" className="mt-6">
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
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-2 flex-wrap">
          {isLoading ? (
            <p className="text-sm text-muted-foreground animate-pulse">{t('providers.ollama.loadingProviders')}</p>
          ) : (
            <span className="text-sm text-muted-foreground">
              {t('providers.servers.registered')}: <span className="font-medium text-foreground">{total}</span>
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
            <Input
              className="h-8 text-sm w-48 pl-8"
              placeholder={t('providers.ollama.searchProvider')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <Button onClick={onRegister} className="shrink-0">
            <Plus className="h-4 w-4 mr-2" />{t('providers.ollama.registerProvider')}
          </Button>
        </div>
      </div>

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

      {!isLoading && providers.length === 0 && !error && (
        <Card className="border-dashed">
          <CardContent className="p-10 text-center text-muted-foreground">
            <Server className="h-10 w-10 mx-auto mb-3 opacity-25" />
            <p className="font-medium">{t('providers.ollama.noBackends')}</p>
            <p className="text-sm mt-1">{t('providers.ollama.noBackendsHint')}</p>
          </CardContent>
        </Card>
      )}

      {providers.length > 0 && (
        <DataTable
          minWidth="800px"
          footer={totalPages > 1 ? (
            <div className="flex items-center justify-between px-6 py-2">
              <span className="text-xs text-muted-foreground">
                {(page - 1) * PAGE_SIZE + 1}–{Math.min(page * PAGE_SIZE, total)} / {total}
              </span>
              <div className="flex items-center gap-1">
                <Button variant="outline" size="icon" className="h-7 w-7"
                  aria-label={t('common.prevPage')}
                  onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={page <= 1}>
                  <ChevronLeft className="h-3.5 w-3.5" />
                </Button>
                <span className="text-xs text-muted-foreground px-1">{page} / {totalPages}</span>
                <Button variant="outline" size="icon" className="h-7 w-7"
                  aria-label={t('common.nextPage')}
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={page >= totalPages}>
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
              <TableHead className="whitespace-nowrap">{t('providers.ollama.status')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
              <TableHead className="text-right whitespace-nowrap">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {providers.map((b) => {
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
