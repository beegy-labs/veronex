'use client'

import { useMemo } from 'react'
import type { Provider } from '@/lib/types'
import { Plus, Trash2, Pencil, Mic } from 'lucide-react'
import { Button } from '@/components/ui/button'
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
import { getWhisperProviders, countByStatus } from '@/lib/utils'
import { StatusBadge, StatusPill } from './shared'

// ── Tab: Whisper STT providers ──────────────────────────────────────────────────

export function WhisperTab({
  providers,
  isLoading,
  error,
  onRegister,
  onEdit,
  onDelete,
  deleteIsPending,
}: {
  providers: Provider[] | undefined
  isLoading: boolean
  error: Error | null
  onRegister: () => void
  onEdit: (b: Provider) => void
  onDelete: (id: string, name: string) => void
  deleteIsPending: boolean
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const whisper = useMemo(() => getWhisperProviders(providers), [providers])
  const whisperCounts = useMemo(() => countByStatus(whisper), [whisper])
  const onlineCount = whisperCounts['online'] ?? 0
  const degradedCount = whisperCounts['degraded'] ?? 0
  const offlineCount = whisperCounts['offline'] ?? 0

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-text-bright">{t('providers.whisper.title')}</h2>
            {providers ? (
              <div className="flex items-center gap-2 flex-wrap mt-1.5">
                <StatusPill icon={<Mic className="h-3 w-3 shrink-0" />} count={whisper.length} label={t('providers.servers.registered')} />
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
            <Plus className="h-4 w-4 mr-2" />{t('providers.whisper.registerProvider')}
          </Button>
        </div>

        {isLoading && (
          <div className="flex h-32 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('providers.whisper.loadingProviders')}
          </div>
        )}

        {error && (
          <Card className="border-destructive/40 bg-destructive/5">
            <CardContent className="p-5 text-destructive">
              <p className="font-semibold">{t('providers.whisper.failedProviders')}</p>
              <p className="text-sm mt-1 opacity-75">
                {error instanceof Error ? error.message : t('common.unknownError')}
              </p>
            </CardContent>
          </Card>
        )}

        {!isLoading && whisper.length === 0 && !error && (
          <Card className="border-dashed">
            <CardContent className="p-10 text-center text-muted-foreground">
              <Mic className="h-10 w-10 mx-auto mb-3 opacity-25" />
              <p className="font-medium text-text-dim">{t('providers.whisper.noProviders')}</p>
              <p className="text-sm mt-1 text-muted-foreground/70">{t('providers.whisper.noProvidersHint')}</p>
            </CardContent>
          </Card>
        )}

        {whisper.length > 0 && (
          <DataTable minWidth="640px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('providers.whisper.name')}</TableHead>
                <TableHead>{t('providers.ollama.url')}</TableHead>
                <TableHead>{t('providers.whisper.status')}</TableHead>
                <TableHead>{t('providers.whisper.registeredAt')}</TableHead>
                <TableHead className="text-right">{t('keys.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {whisper.map((b) => (
                <TableRow key={b.id}>
                  <TableCell>
                    <div className="font-semibold text-text-bright">{b.name}</div>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground font-mono">{b.url}</TableCell>
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
                              className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10"
                              aria-label={t('providers.whisper.editTitle')}
                              onClick={() => onEdit(b)}>
                              <Pencil className="h-4 w-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>{t('providers.whisper.editTitle')}</TooltipContent>
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
    </div>
  )
}
