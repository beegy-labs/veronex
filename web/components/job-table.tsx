'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { Job, JobDetail, RetryParams, ChatMessage } from '@/lib/types'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { RotateCcw, X, Loader2, Info, Wrench, ChevronDown, ChevronRight } from 'lucide-react'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable, DataTableEmpty } from '@/components/data-table'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useTranslation } from '@/i18n'
import { fmtMsNullable } from '@/lib/chart-theme'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime, fmtNumber } from '@/lib/date'

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

// fmtMsNullable imported from chart-theme — handles ms/s/m/h tiers
const formatDuration = fmtMsNullable

// ── Job detail modal ───────────────────────────────────────────────────────────

// ── Role badge styling ──────────────────────────────────────────────────────────

const ROLE_STYLE: Record<string, string> = {
  system:    'bg-muted text-muted-foreground border-border',
  user:      'bg-status-info/10 text-status-info-fg border-status-info/30',
  assistant: 'bg-status-success/10 text-status-success-fg border-status-success/30',
  tool:      'bg-status-warning/10 text-status-warning-fg border-status-warning/30',
}

function ConversationHistory({ messages }: { messages: ChatMessage[] }) {
  const [open, setOpen] = useState(false)
  const { t } = useTranslation()

  return (
    <div className="border-t border-border">
      <button
        className="w-full flex items-center gap-2 px-6 py-3 text-xs font-semibold tracking-wider uppercase text-muted-foreground hover:text-foreground hover:bg-accent/30 transition-colors text-left"
        onClick={() => setOpen(v => !v)}
      >
        {open ? <ChevronDown className="h-3.5 w-3.5 shrink-0" /> : <ChevronRight className="h-3.5 w-3.5 shrink-0" />}
        {t('jobs.conversationHistory')} ({messages.length})
      </button>
      {open && (
        <div className="px-6 pb-4 space-y-2 max-h-80 overflow-y-auto">
          {messages.map((msg, i) => (
            <div key={i} className={`rounded-md border px-3 py-2 ${ROLE_STYLE[msg.role] ?? ROLE_STYLE.system}`}>
              <div className="flex items-center gap-2 mb-1">
                <span className="text-[10px] font-mono font-bold uppercase tracking-wider">{msg.role}</span>
                {msg.name && (
                  <span className="text-[10px] font-mono text-muted-foreground">({msg.name})</span>
                )}
                {msg.tool_call_id && (
                  <span className="text-[10px] font-mono text-muted-foreground ml-auto">{msg.tool_call_id}</span>
                )}
              </div>
              {msg.content != null ? (
                <pre className="text-xs font-mono whitespace-pre-wrap break-words text-foreground/80 max-h-24 overflow-y-auto">
                  {msg.content}
                </pre>
              ) : msg.tool_calls && msg.tool_calls.length > 0 ? (
                <div className="space-y-1">
                  {msg.tool_calls.map((tc, j) => (
                    <div key={tc.id ?? j} className="flex items-center gap-1.5 text-xs font-mono">
                      <Wrench className="h-3 w-3 shrink-0" />
                      <span className="font-semibold">{tc.function?.name}</span>
                      {tc.id && <span className="text-muted-foreground text-[10px]">{tc.id}</span>}
                    </div>
                  ))}
                </div>
              ) : (
                <span className="text-xs text-muted-foreground italic">(empty)</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

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
  const { tz } = useTimezone()
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
                  {data.model_name} · {data.provider_type}
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
                <MetaItem label={t('jobs.createdAt')}   value={fmtDatetime(data.created_at, tz)} />
                <MetaItem label={t('jobs.startedAt')}   value={data.started_at   ? fmtDatetime(data.started_at, tz)   : '—'} />
                <MetaItem label={t('jobs.completedAt')} value={data.completed_at ? fmtDatetime(data.completed_at, tz) : '—'} />
                <MetaItem label={t('jobs.latency')}     value={formatDuration(data.latency_ms)} />
                <MetaItem label={t('jobs.ttft')}        value={formatDuration(data.ttft_ms)} />
                <MetaItem
                  label={t('jobs.tps')}
                  value={data.tps != null ? `${data.tps.toFixed(1)} tok/s` : '—'}
                />
                <MetaItem
                  label={t('jobs.promptTokens')}
                  value={data.prompt_tokens != null ? fmtNumber(data.prompt_tokens) : '—'}
                  tooltip={t('jobs.promptTokensTooltip')}
                />
                <MetaItem
                  label={t('jobs.completionTokens')}
                  value={data.completion_tokens != null ? fmtNumber(data.completion_tokens) : '—'}
                />
                {data.cached_tokens != null && data.cached_tokens > 0 && (
                  <MetaItem
                    label={t('jobs.cachedTokens')}
                    value={fmtNumber(data.cached_tokens)}
                  />
                )}
                {(data.prompt_tokens != null && data.completion_tokens != null) && (
                  <MetaItem
                    label={t('jobs.totalTokens')}
                    value={fmtNumber(data.prompt_tokens + data.completion_tokens)}
                  />
                )}
                {data.api_key_name && (
                  <MetaItem label={t('jobs.apiKey')} value={data.api_key_name} accent />
                )}
                {data.account_name && (
                  <MetaItem label={t('test.runner')} value={data.account_name} accent />
                )}
                {data.request_path && (
                  <MetaItem label={t('jobs.endpoint')} value={data.request_path} />
                )}
                {data.message_count != null && data.message_count > 1 && (
                  <MetaItem label={t('jobs.conversationTurns')} value={String(data.message_count)} />
                )}
                {data.estimated_cost_usd != null && (
                  <MetaItem
                    label={t('jobs.estimatedCost')}
                    value={data.estimated_cost_usd === 0
                      ? '$0.00 (self-hosted)'
                      : `$${data.estimated_cost_usd.toFixed(6)}`}
                    accent={data.estimated_cost_usd > 0}
                  />
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
              ) : data.tool_calls_json && data.tool_calls_json.length > 0 && !data.result_text ? (
                <div className="px-6 py-4">
                  <p className="text-xs font-semibold tracking-wider uppercase mb-2 text-status-info-fg">
                    {t('jobs.toolCalls')}
                  </p>
                  <p className="text-xs text-muted-foreground mb-3">{t('jobs.agentToolCall')}</p>
                  <div className="space-y-2">
                    {data.tool_calls_json.map((tc, i) => (
                      <div key={tc.id ?? i} className="rounded-md border border-border bg-muted/40 px-3 py-2">
                        <div className="flex items-center gap-2 mb-1">
                          <Wrench className="h-3.5 w-3.5 text-status-info-fg shrink-0" />
                          <code className="text-xs font-mono font-semibold text-status-info-fg">
                            {tc.function?.name ?? 'unknown'}
                          </code>
                          {tc.id && (
                            <span className="text-[10px] text-muted-foreground font-mono ml-auto">{tc.id}</span>
                          )}
                        </div>
                        {tc.function?.arguments && (
                          <pre className="text-xs font-mono text-foreground/75 whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
                            {typeof tc.function.arguments === 'string'
                              ? tc.function.arguments
                              : JSON.stringify(tc.function.arguments, null, 2)}
                          </pre>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
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

              {/* Conversation history — collapsible, only when messages exist */}
              {data.messages_json && data.messages_json.length > 0 && (
                <ConversationHistory messages={data.messages_json} />
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
                onRetry?.({ prompt: data.prompt, model: data.model_name, provider_type: data.provider_type })
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

function MetaItem({ label, value, accent, tooltip }: { label: string; value: string; accent?: boolean; tooltip?: string }) {
  return (
    <div>
      {tooltip ? (
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="text-muted-foreground inline-flex items-center gap-0.5 cursor-default">
                {label}
                <Info className="h-3 w-3 shrink-0" />:
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">{tooltip}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      ) : (
        <span className="text-muted-foreground">{label}: </span>
      )}
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
  const { tz } = useTimezone()
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
            <TableHead>{t('jobs.endpoint')}</TableHead>
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
              <TableCell className="text-muted-foreground">{job.provider_type}</TableCell>
              <TableCell className="text-xs text-primary/80">
                {job.source === 'test'
                  ? (job.account_name ?? <span className="text-muted-foreground">—</span>)
                  : (job.api_key_name ?? <span className="text-muted-foreground">—</span>)}
              </TableCell>
              <TableCell className="font-mono text-xs text-muted-foreground max-w-[160px] truncate" title={job.request_path ?? undefined}>
                {job.request_path ?? <span className="opacity-40">—</span>}
              </TableCell>
              <TableCell>
                <div className="flex items-center gap-1.5">
                  <StatusBadge status={job.status} />
                  {job.has_tool_calls && (
                    <span title="Tool calls">
                      <Wrench className="h-3 w-3 text-status-info-fg shrink-0" />
                    </span>
                  )}
                </div>
              </TableCell>
              <TableCell className="text-xs text-muted-foreground">
                {fmtDatetime(job.created_at, tz)}
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
