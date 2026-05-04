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
    <div className="vds-space-y-8">
      <div className="vds-space-y-4">
        <div className="vds-flex vds-items-start vds-justify-between">
          <div>
            <h2 className="vds-text-base vds-font-600 vds-text-bright">{t('providers.gemini.title')}</h2>
            {providers ? (
              <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap vds-mt-1.5">
                <StatusPill icon={<Key className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={gemini.length} label={t('providers.servers.registered')} />
                {onlineCount > 0 && (
                  <StatusPill
                    icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-flex-shrink-0" />}
                    count={onlineCount} label={t('common.online')}
                    className="vds-bg-success/10 vds-border-1 vds-border-success/30 vds-text-success"
                  />
                )}
                {degradedCount > 0 && (
                  <StatusPill
                    icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-warning vds-flex-shrink-0" />}
                    count={degradedCount} label={t('common.degraded')}
                    className="vds-bg-warning/10 vds-border-1 vds-border-warning/30 vds-text-warning"
                  />
                )}
                {offlineCount > 0 && (
                  <StatusPill
                    icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-flex-shrink-0" />}
                    count={offlineCount} label={t('common.offline')}
                    className="vds-bg-error/10 vds-border-1 vds-border-error/30 vds-text-error"
                  />
                )}
              </div>
            ) : (
              <p className="vds-text-sm vds-text-dim vds-mt-0.5 vds-animate-pulse">{t('common.loading')}</p>
            )}
          </div>
          <Button onClick={onRegister} className="vds-flex-shrink-0">
            <Plus className="vds-h-4 vds-w-4 vds-mr-2" />{t('providers.gemini.registerProvider')}
          </Button>
        </div>

        {isLoading && (
          <div className="vds-flex vds-h-32 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('providers.gemini.loadingProviders')}
          </div>
        )}

        {error && (
          <Card className="vds-border-destructive/40 vds-bg-destructive/5">
            <CardContent className="vds-p-5 vds-text-destructive">
              <p className="vds-font-600">{t('providers.gemini.failedProviders')}</p>
              <p className="vds-text-sm vds-mt-1 vds-opacity-75">
                {error instanceof Error ? error.message : t('common.unknownError')}
              </p>
            </CardContent>
          </Card>
        )}

        {!isLoading && gemini.length === 0 && !error && (
          <Card className="vds-border-dashed">
            <CardContent className="vds-p-10 vds-text-center vds-text-dim">
              <Key className="vds-h-10 vds-w-10 vds-mx-auto vds-mb-3 vds-opacity-25" />
              <p className="vds-font-500 vds-text-dim">{t('providers.gemini.noBackends')}</p>
              <p className="vds-text-sm vds-mt-1 vds-text-dim/70">{t('providers.gemini.noBackendsHint')}</p>
            </CardContent>
          </Card>
        )}

        {gemini.length > 0 && (
          <DataTable
            minWidth="760px"
            footer={geminiTotalPages > 1 ? (
              <div className="vds-flex vds-items-center vds-justify-between vds-px-6 vds-py-2">
                <span className="vds-text-xs vds-text-dim">
                  {geminiPageStart + 1}–{Math.min(geminiPageStart + PAGE_SIZE, gemini.length)} / {gemini.length}
                </span>
                <div className="vds-flex vds-items-center vds-gap-1">
                  <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                    aria-label={t('common.prevPage')}
                    onClick={() => setGeminiPage((p) => Math.max(1, p - 1))} disabled={geminiSafePage <= 1}>
                    <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
                  </Button>
                  <span className="vds-text-xs vds-text-dim vds-px-1">{geminiSafePage} / {geminiTotalPages}</span>
                  <Button variant="outline" size="icon" className="vds-h-7 vds-w-7"
                    aria-label={t('common.nextPage')}
                    onClick={() => setGeminiPage((p) => Math.min(geminiTotalPages, p + 1))} disabled={geminiSafePage >= geminiTotalPages}>
                    <ChevronRight className="vds-h-3.5 vds-w-3.5" />
                  </Button>
                </div>
              </div>
            ) : undefined}
          >
            <TableHeader>
              <TableRow className="vds-hover:bg-transparent">
                <TableHead className="vds-whitespace-nowrap">{t('providers.gemini.name')}</TableHead>
                <TableHead className="vds-whitespace-nowrap">{t('providers.gemini.apiKey')}</TableHead>
                <TableHead className="vds-whitespace-nowrap">{t('providers.gemini.freeTier')}</TableHead>
                <TableHead className="vds-whitespace-nowrap">{t('providers.gemini.status')}</TableHead>
                <TableHead className="vds-whitespace-nowrap">{t('providers.servers.registeredAt')}</TableHead>
                <TableHead className="vds-text-right vds-whitespace-nowrap">{t('keys.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {geminiPageItems.map((b) => (
                <TableRow key={b.id}>
                  <TableCell>
                    <div className="vds-font-600 vds-text-bright">{b.name}</div>
                  </TableCell>
                  <TableCell>
                    <ApiKeyCell providerId={b.id} masked={b.api_key_masked} />
                  </TableCell>
                  <TableCell>
                    {b.is_free_tier ? (
                      <Badge variant="outline" className="vds-bg-warning/15 vds-text-warning vds-border-warning/30 vds-text-[10px] vds-px-2 vds-py-0.5 vds-whitespace-nowrap">
                        {t('providers.gemini.freeTier')}
                      </Badge>
                    ) : (
                      <Badge variant="outline" className="vds-bg-success/15 vds-text-success vds-border-success/30 vds-text-[10px] vds-px-2 vds-py-0.5 vds-whitespace-nowrap">
                        {t('providers.gemini.paid')}
                      </Badge>
                    )}
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
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-text-bright"
                              aria-label={t('common.sync')}
                              onClick={() => onSync(b.id)}
                              disabled={syncPending}>
                              <RefreshCw className="vds-h-4 vds-w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('common.sync')}</TooltipContent>
                        </Tooltip>
                        {!b.is_free_tier && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <Button variant="ghost" size="icon"
                                className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-accent-gpu vds-hover:bg-hover-gpu/10"
                                aria-label={t('providers.gemini.modelSelection')}
                                onClick={() => setModelSelectionProvider(b)}>
                                <ListFilter className="vds-h-4 vds-w-4" />
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>{t('providers.gemini.modelSelection')}</TooltipContent>
                          </Tooltip>
                        )}
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button variant="ghost" size="icon"
                              className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-primary vds-hover:bg-primary/10"
                              aria-label={t('providers.gemini.editTitle')}
                              onClick={() => onEdit(b)}>
                              <Pencil className="vds-h-4 vds-w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.gemini.editTitle')}</TooltipContent>
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
