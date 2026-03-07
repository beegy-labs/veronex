'use client'

import { useState } from 'react'
import type { Provider } from '@/lib/types'
import { Plus, Trash2, RefreshCw, Key, ShieldCheck, ListFilter, Pencil, ChevronLeft, ChevronRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
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
import { PROVIDER_GEMINI } from '@/lib/constants'
import { StatusBadge } from './shared'
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
  onToggleActive,
  toggleActivePending,
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
  onToggleActive: (b: Provider) => void
  toggleActivePending: boolean
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const gemini = providers?.filter((b) => b.provider_type === PROVIDER_GEMINI) ?? []
  const geminiCounts = gemini.reduce((acc, b) => {
    acc[b.status] = (acc[b.status] ?? 0) + 1
    if (b.is_active) acc['_active'] = (acc['_active'] ?? 0) + 1
    return acc
  }, {} as Record<string, number>)
  const onlineCount = geminiCounts['online'] ?? 0
  const activeCount = geminiCounts['_active'] ?? 0
  const degradedCount = geminiCounts['degraded'] ?? 0
  const offlineCount = geminiCounts['offline'] ?? 0
  const [modelSelectionProvider, setModelSelectionProvider] = useState<Provider | null>(null)
  const [geminiPage, setGeminiPage] = useState(1)
  const geminiTotalPages = Math.max(1, Math.ceil(gemini.length / PAGE_SIZE))
  const geminiSafePage = Math.min(geminiPage, geminiTotalPages)
  const geminiPageStart = (geminiSafePage - 1) * PAGE_SIZE
  const geminiPageItems = gemini.slice(geminiPageStart, geminiPageStart + PAGE_SIZE)

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-text-bright">{t('providers.gemini.title')}</h2>
            {providers ? (
              <div className="flex items-center gap-2 flex-wrap mt-1.5">
                <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted/60 border border-border text-xs font-medium text-muted-foreground">
                  <Key className="h-3 w-3 shrink-0" />
                  <span className="tabular-nums">{gemini.length}</span>
                  <span>{t('providers.servers.registered')}</span>
                </div>
                {activeCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 border border-primary/30 text-xs font-medium text-primary">
                    <ShieldCheck className="h-3 w-3 shrink-0" />
                    <span className="tabular-nums">{activeCount}</span>
                    <span>{t('common.active')}</span>
                  </div>
                )}
                {onlineCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-success/10 border border-status-success/30 text-xs font-medium text-status-success-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
                    <span className="tabular-nums">{onlineCount}</span>
                    <span>{t('common.online')}</span>
                  </div>
                )}
                {degradedCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-warn/10 border border-status-warn/30 text-xs font-medium text-status-warn-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-warn shrink-0" />
                    <span className="tabular-nums">{degradedCount}</span>
                    <span>{t('common.degraded')}</span>
                  </div>
                )}
                {offlineCount > 0 && (
                  <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-error/10 border border-status-error/30 text-xs font-medium text-status-error-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
                    <span className="tabular-nums">{offlineCount}</span>
                    <span>{t('common.offline')}</span>
                  </div>
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
            {t('providers.gemini.loadingBackends')}
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">{t('providers.gemini.failedBackends')}</p>
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
                    onClick={() => setGeminiPage((p) => Math.max(1, p - 1))} disabled={geminiSafePage <= 1}>
                    <ChevronLeft className="h-3.5 w-3.5" />
                  </Button>
                  <span className="text-xs text-muted-foreground px-1">{geminiSafePage} / {geminiTotalPages}</span>
                  <Button variant="outline" size="icon" className="h-7 w-7"
                    onClick={() => setGeminiPage((p) => Math.min(geminiTotalPages, p + 1))} disabled={geminiSafePage >= geminiTotalPages}>
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ) : undefined}
          >
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('providers.gemini.name')}</TableHead>
                <TableHead>{t('providers.gemini.apiKey')}</TableHead>
                <TableHead className="w-24">{t('providers.gemini.freeTier')}</TableHead>
                <TableHead className="w-24">{t('providers.gemini.activeToggle')}</TableHead>
                <TableHead className="w-28">{t('providers.gemini.status')}</TableHead>
                <TableHead className="w-32">{t('providers.servers.registeredAt')}</TableHead>
                <TableHead className="text-right w-28">{t('keys.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {geminiPageItems.map((b) => (
                <TableRow key={b.id} className={!b.is_active ? 'opacity-50' : ''}>
                  <TableCell>
                    <div className="font-semibold text-text-bright">{b.name}</div>
                  </TableCell>
                  <TableCell>
                    <ApiKeyCell providerId={b.id} masked={b.api_key_masked} />
                  </TableCell>
                  <TableCell>
                    {b.is_free_tier ? (
                      <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                        {t('providers.gemini.freeTier')}
                      </Badge>
                    ) : (
                      <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 text-[10px] px-1.5 py-0">
                        {t('providers.gemini.paid')}
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={b.is_active}
                      onCheckedChange={() => onToggleActive(b)}
                      disabled={toggleActivePending}
                      title={b.is_active ? t('providers.disableProvider') : t('providers.enableProvider')}
                    />
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
                              onClick={() => onSync(b.id)}
                              disabled={syncPending}>
                              <RefreshCw className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>Sync</TooltipContent>
                        </Tooltip>
                        {!b.is_free_tier && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <Button variant="ghost" size="icon"
                                className="h-8 w-8 text-muted-foreground hover:text-accent-gpu hover:bg-accent-gpu/10"
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
