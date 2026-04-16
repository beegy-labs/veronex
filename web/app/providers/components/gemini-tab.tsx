'use client'

import { useState, useMemo } from 'react'
import type { Provider } from '@/lib/types'
import { Plus, Trash2, RefreshCw, Key, ListFilter, Pencil, ChevronLeft, ChevronRight } from 'lucide-react'
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
import { getGeminiProviders, countByStatus } from '@/lib/utils'
import { StatusBadge, StatusPill } from './shared'
import { ApiKeyCell, ModelSelectionModal } from './modals'
import { PAGE_SIZE } from './ollama-sections'
import { GeminiStatusSyncSection, GeminiSyncSection } from './gemini-sections'

// ── Tab: Gemini providers + policies ───────────────────────────────────────────

export function GeminiTab({
  providers,
  isLoading,
  error,
  onRegister,
  onEdit,
  onSync,
  syncPending,
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
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const gemini = useMemo(() => getGeminiProviders(providers), [providers])
  const geminiCounts = useMemo(() => countByStatus(gemini), [gemini])
  const onlineCount = geminiCounts['online'] ?? 0
  const degradedCount = geminiCounts['degraded'] ?? 0
  const offlineCount = geminiCounts['offline'] ?? 0
  const [modelSelectionProvider, setModelSelectionProvider] = useState<Provider | null>(null)
  const [geminiPage, setGeminiPage] = useState(1)
  const { geminiTotalPages, geminiSafePage, geminiPageStart, geminiPageItems } = useMemo(() => {
    const geminiTotalPages = Math.max(1, Math.ceil(gemini.length / PAGE_SIZE))
    const geminiSafePage = Math.min(geminiPage, geminiTotalPages)
    const geminiPageStart = (geminiSafePage - 1) * PAGE_SIZE
    const geminiPageItems = gemini.slice(geminiPageStart, geminiPageStart + PAGE_SIZE)
    return { geminiTotalPages, geminiSafePage, geminiPageStart, geminiPageItems }
  }, [gemini, geminiPage])

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-text-bright">{t('providers.gemini.title')}</h2>
            {providers ? (
              <div className="flex items-center gap-2 flex-wrap mt-1.5">
                <StatusPill icon={<Key className="h-3 w-3 shrink-0" />} count={gemini.length} label={t('providers.servers.registered')} />
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
              <p className="text-sm text-muted-foreground mt-0.5 animate-pulse">{t('common.loading')}</p>
            )}
          </div>
          <Button onClick={onRegister} className="shrink-0">
            <Plus className="h-4 w-4 mr-2" />{t('providers.gemini.registerProvider')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('providers.gemini.loadingProviders')}
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">{t('providers.gemini.failedProviders')}</p>
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
                    aria-label={t('common.prevPage')}
                    onClick={() => setGeminiPage((p) => Math.max(1, p - 1))} disabled={geminiSafePage <= 1}>
                    <ChevronLeft className="h-3.5 w-3.5" />
                  </Button>
                  <span className="text-xs text-muted-foreground px-1">{geminiSafePage} / {geminiTotalPages}</span>
                  <Button variant="outline" size="icon" className="h-7 w-7"
                    aria-label={t('common.nextPage')}
                    onClick={() => setGeminiPage((p) => Math.min(geminiTotalPages, p + 1))} disabled={geminiSafePage >= geminiTotalPages}>
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ) : undefined}
          >
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="whitespace-nowrap">{t('providers.gemini.name')}</TableHead>
                <TableHead className="whitespace-nowrap">{t('providers.gemini.apiKey')}</TableHead>
                <TableHead className="whitespace-nowrap">{t('providers.gemini.freeTier')}</TableHead>
                <TableHead className="whitespace-nowrap">{t('providers.gemini.status')}</TableHead>
                <TableHead className="whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
                <TableHead className="text-right whitespace-nowrap">{t('keys.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {geminiPageItems.map((b) => (
                <TableRow key={b.id}>
                  <TableCell>
                    <div className="font-semibold text-text-bright">{b.name}</div>
                  </TableCell>
                  <TableCell>
                    <ApiKeyCell providerId={b.id} masked={b.api_key_masked} />
                  </TableCell>
                  <TableCell>
                    {b.is_free_tier ? (
                      <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-2 py-0.5 whitespace-nowrap">
                        {t('providers.gemini.freeTier')}
                      </Badge>
                    ) : (
                      <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 text-[10px] px-2 py-0.5 whitespace-nowrap">
                        {t('providers.gemini.paid')}
                      </Badge>
                    )}
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
                              aria-label={t('common.sync')}
                              onClick={() => onSync(b.id)}
                              disabled={syncPending}>
                              <RefreshCw className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('common.sync')}</TooltipContent>
                        </Tooltip>
                        {!b.is_free_tier && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <Button variant="ghost" size="icon"
                                className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
                                aria-label={t('providers.gemini.modelSelection')}
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
                              aria-label={t('providers.gemini.editTitle')}
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
