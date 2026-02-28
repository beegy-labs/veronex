'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { Job, JobDetail, RetryParams } from '@/lib/types'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { RotateCcw, X, Loader2 } from 'lucide-react'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable, DataTableEmpty } from '@/components/data-table'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'
import { fmtMsNullable } from '@/lib/chart-theme'

// ── Status styling ─────────────────────────────────────────────────────────────

const STATUS_EXTRA: Record<string, string> = {
  completed: 'bg-status-success/15 text-status-success-fg border-status-success/30',
  failed:    'bg-status-error/15 text-status-error-fg border-status-error/30',
  cancelled: 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30',
  pending:   'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  running:   'bg-status-info/15 text-status-info-fg border-status-info/30',
}

function StatusBadge({ status }: { status: string }) {
  return (
    <Badge
      variant="outline"
      className={STATUS_EXTRA[status] ?? 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30'}
    >
      {status}
    </Badge>
  )
}

// ── Formatters ─────────────────────────────────────────────────────────────────

function truncateId(id: string) {
  return id.slice(0, 8) + '…'
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  })
}

// fmtMsNullable imported from chart-theme — handles ms/s/m/h tiers
const formatDuration = fmtMsNullable

// ── Job detail modal ───────────────────────────────────────────────────────────

function JobDetailModal({
  jobId,
  open,
  onClose,
  onRetry,
}: {
  jobId: string | null
  open: boolean
  onClose: () => void
  onRetry?: (params: RetryParams) => void
}) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const { data, isLoading } = useQuery<JobDetail>({
    queryKey: ['job-detail', jobId],
    queryFn: () => api.jobDetail(jobId!),
    enabled: !!jobId && open,
    staleTime: 30_000,
  })

  const cancelMutation = useMutation({
    mutationFn: () => api.cancelJob(jobId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
      queryClient.invalidateQueries({ queryKey: ['job-detail', jobId] })
    },
  })

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) onClose() }}>
      <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col gap-0 p-0 overflow-hidden">
        <DialogHeader className="px-6 pt-5 pb-4 border-b border-border shrink-0">
          <DialogTitle className="flex items-center gap-3 flex-wrap">
            {data ? (
              <>
                <span className="font-mono text-xs text-muted-foreground">{data.id}</span>
                <StatusBadge status={data.status} />
                <span className="text-sm font-normal text-muted-foreground">
                  {data.model_name} · {data.backend}
                </span>
              </>
            ) : (
              <span className="text-muted-foreground text-sm">{t('common.loading')}</span>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="overflow-y-auto flex-1">
          {isLoading && (
            <div className="p-6 text-center text-muted-foreground text-sm">{t('common.loading')}</div>
          )}

          {data && (
            <div className="flex flex-col gap-0 divide-y divide-border">
              {/* Timing row */}
              <div className="px-6 py-3 grid grid-cols-3 gap-x-4 gap-y-1 text-xs">
                <MetaItem label={t('jobs.createdAt')}   value={formatDate(data.created_at)} />
                <MetaItem label={t('jobs.startedAt')}   value={data.started_at   ? formatDate(data.started_at)   : '—'} />
                <MetaItem label={t('jobs.completedAt')} value={data.completed_at ? formatDate(data.completed_at) : '—'} />
                <MetaItem label={t('jobs.latency')}     value={formatDuration(data.latency_ms)} />
                <MetaItem label={t('jobs.ttft')}        value={formatDuration(data.ttft_ms)} />
                <MetaItem
                  label={t('jobs.tps')}
                  value={data.tps != null ? `${data.tps.toFixed(1)} tok/s` : '—'}
                />
                <MetaItem
                  label={t('jobs.promptTokens')}
                  value={data.prompt_tokens != null ? data.prompt_tokens.toLocaleString() : '—'}
                />
                <MetaItem
                  label={t('jobs.completionTokens')}
                  value={data.completion_tokens != null ? data.completion_tokens.toLocaleString() : '—'}
                />
                {data.cached_tokens != null && data.cached_tokens > 0 && (
                  <MetaItem
                    label={t('jobs.cachedTokens')}
                    value={data.cached_tokens.toLocaleString()}
                  />
                )}
                {(data.prompt_tokens != null && data.completion_tokens != null) && (
                  <MetaItem
                    label={t('jobs.totalTokens')}
                    value={(data.prompt_tokens + data.completion_tokens).toLocaleString()}
                  />
                )}
                {data.api_key_name && (
                  <MetaItem label={t('jobs.apiKey')} value={data.api_key_name} accent />
                )}
              </div>

              {/* Prompt */}
              <TextSection
                label={t('jobs.prompt')}
                text={data.prompt || '(empty)'}
                labelClass="text-accent-brand"
              />

              {/* Result or error */}
              {data.status === 'failed' ? (
                <TextSection
                  label={t('jobs.error')}
                  text={data.error || t('jobs.noError')}
                  labelClass="text-status-error-fg"
                  textClass="text-status-error-fg/80"
                />
              ) : (
                <TextSection
                  label={t('jobs.result')}
                  text={data.result_text || (
                    data.status === 'completed'
                      ? t('jobs.noResult')
                      : data.status === 'running'
                        ? t('jobs.processing')
                        : `(${t('jobs.statuses.pending')})`
                  )}
                  labelClass="text-status-success-fg"
                />
              )}
            </div>
          )}
        </div>

        {/* Action footer */}
        {data && (
          <div className="shrink-0 border-t border-border px-6 py-3 flex items-center justify-between gap-2 flex-wrap">
            {/* Retry in Test — re-runs job in the test panel above the table */}
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                onRetry?.({ prompt: data.prompt, model: data.model_name, backend: data.backend })
                onClose()
              }}
            >
              <RotateCcw className="h-3.5 w-3.5 mr-1.5" />
              {t('jobs.retryInTest')}
            </Button>

            {/* Cancel — only for pending / running jobs */}
            {(data.status === 'pending' || data.status === 'running') && (
              <Button
                size="sm"
                variant="outline"
                className="text-destructive border-destructive/40 hover:bg-destructive/10"
                onClick={() => cancelMutation.mutate()}
                disabled={cancelMutation.isPending}
              >
                {cancelMutation.isPending
                  ? <><Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />{t('jobs.cancelling')}</>
                  : <><X className="h-3.5 w-3.5 mr-1.5" />{t('jobs.cancelJob')}</>}
              </Button>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

function MetaItem({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div>
      <span className="text-muted-foreground">{label}: </span>
      <span className={`tabular-nums ${accent ? 'text-primary' : 'text-foreground'}`}>{value}</span>
    </div>
  )
}

function TextSection({
  label,
  text,
  labelClass = '',
  textClass = '',
}: {
  label: string
  text: string
  labelClass?: string
  textClass?: string
}) {
  return (
    <div className="px-6 py-4">
      <p className={`text-xs font-semibold tracking-wider uppercase mb-2 ${labelClass}`}>
        {label}
      </p>
      <pre
        className={`text-sm font-mono whitespace-pre-wrap break-words leading-relaxed text-foreground/85 max-h-52 overflow-y-auto ${textClass}`}
      >
        {text}
      </pre>
    </div>
  )
}

// ── Job table ──────────────────────────────────────────────────────────────────

export default function JobTable({
  jobs,
  onRetry,
}: {
  jobs: Job[]
  onRetry?: (params: RetryParams) => void
}) {
  const { t } = useTranslation()
  const [selectedId, setSelectedId] = useState<string | null>(null)

  if (jobs.length === 0) {
    return <DataTableEmpty>{t('jobs.noJobsFound')}</DataTableEmpty>
  }

  return (
    <>
      <DataTable minWidth="760px">
        <TableHeader>
          <TableRow>
            <TableHead>{t('jobs.id')}</TableHead>
            <TableHead>{t('jobs.model')}</TableHead>
            <TableHead>{t('jobs.backend')}</TableHead>
            <TableHead>{t('jobs.apiKey')}</TableHead>
            <TableHead>{t('jobs.status')}</TableHead>
            <TableHead>{t('jobs.createdAt')}</TableHead>
            <TableHead className="text-right">{t('jobs.ttft')}</TableHead>
            <TableHead className="text-right">{t('jobs.latency')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {jobs.map((job) => (
            <TableRow
              key={job.id}
              className="cursor-pointer hover:bg-accent/50"
              onClick={() => setSelectedId(job.id)}
            >
              <TableCell className="font-mono text-xs text-muted-foreground">
                <span title={job.id}>{truncateId(job.id)}</span>
              </TableCell>
              <TableCell>{job.model_name}</TableCell>
              <TableCell className="text-muted-foreground">{job.backend}</TableCell>
              <TableCell className="text-xs text-primary/80">
                {job.api_key_name ?? <span className="text-muted-foreground">—</span>}
              </TableCell>
              <TableCell><StatusBadge status={job.status} /></TableCell>
              <TableCell className="text-xs text-muted-foreground">
                {formatDate(job.created_at)}
              </TableCell>
              <TableCell className="text-right tabular-nums text-muted-foreground text-xs">
                {formatDuration(job.ttft_ms)}
              </TableCell>
              <TableCell className="text-right tabular-nums text-muted-foreground">
                {formatDuration(job.latency_ms)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </DataTable>

      <JobDetailModal
        jobId={selectedId}
        open={!!selectedId}
        onClose={() => setSelectedId(null)}
        onRetry={onRetry}
      />
    </>
  )
}
