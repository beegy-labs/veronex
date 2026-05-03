'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { JobDetail, RetryParams, ChatMessage } from '@/lib/types'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { RotateCcw, X, Loader2, Info, Wrench, ChevronDown, ChevronRight } from 'lucide-react'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useTranslation } from '@/i18n'
import { fmtMsNullable, fmtTps, fmtCost6 } from '@/lib/chart-theme'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime, fmtNumber } from '@/lib/date'
import { ROLE_STYLES } from '@/lib/constants'
import { jobDetailQuery, JOB_DETAIL_QUERY_KEY, DASHBOARD_JOBS_QUERY_KEY } from '@/lib/queries/dashboard'
import { StatusBadge } from './status-badge'

const formatDuration = fmtMsNullable

function ConversationHistory({ messages }: { messages: ChatMessage[] }) {
  const [open, setOpen] = useState(false)
  const { t } = useTranslation()

  return (
    <div className="vds-border-t-1 vds-border-subtle">
      <button
        type="button"
        aria-expanded={open}
        className="vds-w-full vds-flex vds-items-center vds-gap-2 vds-px-6 vds-py-3 vds-text-xs vds-font-600 vds-tracking-wider vds-uppercase vds-text-dim vds-hover:text-primary vds-hover:bg-hover/30 vds-transition-colors vds-text-left"
        onClick={() => setOpen(v => !v)}
      >
        {open ? <ChevronDown className="vds-h-3.5 vds-w-3.5 vds-flex-shrink-0" /> : <ChevronRight className="vds-h-3.5 vds-w-3.5 vds-flex-shrink-0" />}
        {t('jobs.conversationHistory')} ({messages.length})
      </button>
      {open && (
        <div className="vds-px-6 vds-pb-4 vds-space-y-2 vds-max-h-80 vds-overflow-y-auto">
          {messages.map((msg, i) => (
            <div key={i} className={`vds-rounded-md vds-border-1 vds-px-3 vds-py-2 ${ROLE_STYLES[msg.role] ?? ROLE_STYLES.system}`}>
              <div className="vds-flex vds-items-center vds-gap-2 vds-mb-1">
                <span className="vds-text-[10px] vds-font-mono vds-font-700 vds-uppercase vds-tracking-wider">{msg.role}</span>
                {msg.name && (
                  <span className="vds-text-[10px] vds-font-mono vds-text-dim">({msg.name})</span>
                )}
                {msg.tool_call_id && (
                  <span className="vds-text-[10px] vds-font-mono vds-text-dim vds-ml-auto">{msg.tool_call_id}</span>
                )}
              </div>
              {msg.content != null ? (
                <pre className="vds-text-xs vds-font-mono vds-whitespace-pre-wrap vds-break-words vds-text-primary/80 vds-max-h-24 vds-overflow-y-auto">
                  {msg.content}
                </pre>
              ) : msg.tool_calls && msg.tool_calls.length > 0 ? (
                <div className="vds-space-y-1">
                  {msg.tool_calls.map((tc, j) => (
                    <div key={tc.id ?? j} className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-font-mono">
                      <Wrench className="vds-h-3 vds-w-3 vds-flex-shrink-0" />
                      <span className="vds-font-600">{tc.function?.name}</span>
                      {tc.id && <span className="vds-text-dim vds-text-[10px]">{tc.id}</span>}
                    </div>
                  ))}
                </div>
              ) : (
                <span className="vds-text-xs vds-text-dim vds-italic">({t('common.empty')})</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function MetaItem({ label, value, accent, tooltip }: { label: string; value: string; accent?: boolean; tooltip?: string }) {
  return (
    <div>
      {tooltip ? (
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="vds-text-dim vds-inline-flex vds-items-center vds-gap-0.5 vds-cursor-default">
                {label}
                <Info className="vds-h-3 vds-w-3 vds-flex-shrink-0" />:
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">{tooltip}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      ) : (
        <span className="vds-text-dim">{label}: </span>
      )}
      <span className={`vds-tabular-nums ${accent ? 'vds-text-primary' : 'vds-text-primary'}`}>{value}</span>
    </div>
  )
}

function TextSection({
  label, text, labelClass = '', textClass = '',
}: {
  label: string; text: string; labelClass?: string; textClass?: string
}) {
  return (
    <div className="vds-px-6 vds-py-4">
      <p className={`vds-text-xs vds-font-600 vds-tracking-wider vds-uppercase vds-mb-2 ${labelClass}`}>{label}</p>
      <pre className={`vds-text-sm vds-font-mono vds-whitespace-pre-wrap vds-break-words vds-leading-relaxed vds-text-primary/85 vds-max-h-52 vds-overflow-y-auto ${textClass}`}>
        {text}
      </pre>
    </div>
  )
}

export function JobDetailModal({
  jobId, open, onClose, onRetry,
}: {
  jobId: string | null; open: boolean; onClose: () => void; onRetry?: (params: RetryParams) => void
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const queryClient = useQueryClient()

  const { data, isLoading } = useQuery(jobDetailQuery(jobId, open))

  const cancelMutation = useMutation({
    mutationFn: () => api.cancelJob(jobId!),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: DASHBOARD_JOBS_QUERY_KEY })
      queryClient.invalidateQueries({ queryKey: ['recent-jobs'] })
      queryClient.invalidateQueries({ queryKey: JOB_DETAIL_QUERY_KEY(jobId!) })
    },
  })

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) onClose() }}>
      <DialogContent className="vds-max-w-2xl vds-max-h-[85vh] vds-flex vds-flex-col vds-gap-0 vds-p-0 vds-overflow-hidden">
        <DialogHeader className="vds-px-6 vds-pt-5 vds-pb-4 vds-border-b-1 vds-border-subtle vds-flex-shrink-0">
          <DialogTitle className="vds-flex vds-items-center vds-gap-3 vds-flex-wrap">
            {data ? (
              <>
                <span className="vds-font-mono vds-text-xs vds-text-dim">{data.id}</span>
                <StatusBadge status={data.status} />
                <span className="vds-text-sm vds-font-400 vds-text-dim">
                  {data.model_name} · {data.provider_name ?? data.provider_type}
                </span>
              </>
            ) : (
              <span className="vds-text-dim vds-text-sm">{t('common.loading')}</span>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-overflow-y-auto vds-flex-1">
          {isLoading && (
            <div className="vds-p-6 vds-text-center vds-text-dim vds-text-sm">{t('common.loading')}</div>
          )}

          {data && (
            <div className="vds-flex vds-flex-col vds-gap-0 vds-divide-y vds-divide-border">
              <div className="vds-px-6 vds-py-3 vds-grid vds-grid-cols-3 vds-gap-x-4 vds-gap-y-1 vds-text-xs">
                <MetaItem label={t('jobs.createdAt')}   value={fmtDatetime(data.created_at, tz)} />
                <MetaItem label={t('jobs.startedAt')}   value={data.started_at   ? fmtDatetime(data.started_at, tz)   : '—'} />
                <MetaItem label={t('jobs.completedAt')} value={data.completed_at ? fmtDatetime(data.completed_at, tz) : '—'} />
                <MetaItem label={t('jobs.latency')}     value={formatDuration(data.latency_ms)} />
                <MetaItem label={t('jobs.ttft')}        value={formatDuration(data.ttft_ms)} />
                <MetaItem label={t('jobs.tps')} value={data.tps != null ? fmtTps(data.tps) : '—'} />
                <MetaItem label={t('jobs.promptTokens')} value={data.prompt_tokens != null ? fmtNumber(data.prompt_tokens) : '—'} tooltip={t('jobs.promptTokensTooltip')} />
                <MetaItem label={t('jobs.completionTokens')} value={data.completion_tokens != null ? fmtNumber(data.completion_tokens) : '—'} />
                {data.cached_tokens != null && data.cached_tokens > 0 && (
                  <MetaItem label={t('jobs.cachedTokens')} value={fmtNumber(data.cached_tokens)} />
                )}
                {(data.prompt_tokens != null && data.completion_tokens != null) && (
                  <MetaItem label={t('jobs.totalTokens')} value={fmtNumber(data.prompt_tokens + data.completion_tokens)} />
                )}
                {data.provider_name && <MetaItem label={t('jobs.providerName')} value={data.provider_name} />}
                {data.api_key_name && <MetaItem label={t('jobs.apiKey')} value={data.api_key_name} accent />}
                {data.account_name && <MetaItem label={t('test.runner')} value={data.account_name} accent />}
                {data.request_path && <MetaItem label={t('jobs.endpoint')} value={data.request_path} />}
                {data.message_count != null && data.message_count > 1 && (
                  <MetaItem label={t('jobs.conversationTurns')} value={String(data.message_count)} />
                )}
                {data.estimated_cost_usd != null && (
                  <MetaItem
                    label={t('jobs.estimatedCost')}
                    value={fmtCost6(data.estimated_cost_usd)}
                    accent={data.estimated_cost_usd > 0}
                  />
                )}
              </div>

              <TextSection label={t('jobs.prompt')} text={data.prompt || `(${t('common.empty')})`} labelClass="vds-text-accent-brand" />

              {data.image_urls && data.image_urls.length > 0 && (
                <div className="vds-px-6 vds-py-4 vds-border-t-1 vds-border-subtle">
                  <p className="vds-text-xs vds-font-600 vds-tracking-wider vds-uppercase vds-mb-2 vds-text-dim">{t('jobs.images')}</p>
                  <div className="vds-flex vds-gap-2 vds-flex-wrap">
                    {data.image_urls
                      .filter(url => url.includes('_thumb'))
                      .map((url, i) => (
                        <a
                          key={url}
                          href={url.replace('_thumb', '')}
                          target="_blank"
                          rel="noopener noreferrer"
                        >
                          {/* eslint-disable-next-line @next/next/no-img-element */}
                          <img
                            src={url}
                            alt={`image ${i}`}
                            className="vds-h-16 vds-w-16 vds-object-cover vds-rounded vds-border-1 vds-border-subtle vds-hover:ring-2 vds-hover:ring-primary/50 vds-transition-shadow"
                          />
                        </a>
                      ))}
                  </div>
                </div>
              )}

              {data.status === 'failed' ? (
                <TextSection label={t('jobs.error')} text={data.error || t('jobs.noError')} labelClass="vds-text-error" textClass="vds-text-error/80" />
              ) : data.tool_calls_json && data.tool_calls_json.length > 0 && !data.result_text ? (
                <div className="vds-px-6 vds-py-4">
                  <p className="vds-text-xs vds-font-600 vds-tracking-wider vds-uppercase vds-mb-2 vds-text-info">{t('jobs.toolCalls')}</p>
                  <p className="vds-text-xs vds-text-dim vds-mb-3">{t('jobs.agentToolCall')}</p>
                  <div className="vds-space-y-2">
                    {data.tool_calls_json.map((tc, i) => (
                      <div key={tc.id ?? i} className="vds-rounded-md vds-border-1 vds-border-subtle vds-bg-muted/40 vds-px-3 vds-py-2">
                        <div className="vds-flex vds-items-center vds-gap-2 vds-mb-1">
                          <Wrench className="vds-h-3.5 vds-w-3.5 vds-text-info vds-flex-shrink-0" />
                          <code className="vds-text-xs vds-font-mono vds-font-600 vds-text-info">{tc.function?.name ?? 'unknown'}</code>
                          {tc.id && <span className="vds-text-[10px] vds-text-dim vds-font-mono vds-ml-auto">{tc.id}</span>}
                        </div>
                        {tc.function?.arguments && (
                          <pre className="vds-text-xs vds-font-mono vds-text-primary/75 vds-whitespace-pre-wrap vds-break-words vds-max-h-32 vds-overflow-y-auto">
                            {typeof tc.function.arguments === 'string' ? tc.function.arguments : JSON.stringify(tc.function.arguments, null, 2)}
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
                    data.status === 'completed' ? t('jobs.noResult')
                      : data.status === 'running' ? t('jobs.processing')
                      : `(${t('jobs.statuses.pending')})`
                  )}
                  labelClass="vds-text-success"
                />
              )}

              {data.messages_json && data.messages_json.length > 0 && (
                <ConversationHistory messages={data.messages_json} />
              )}
            </div>
          )}
        </div>

        {data && (
          <div className="vds-flex-shrink-0 vds-border-t-1 vds-border-subtle vds-px-6 vds-py-3 vds-flex vds-items-center vds-justify-between vds-gap-2 vds-flex-wrap">
            <Button size="sm" variant="outline" onClick={() => { onRetry?.({ prompt: data.prompt, model: data.model_name, provider_type: data.provider_type }); onClose() }}>
              <RotateCcw className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
              {t('jobs.retryInTest')}
            </Button>
            {(data.status === 'pending' || data.status === 'running') && (
              <Button size="sm" variant="outline" className="vds-text-destructive vds-border-destructive/40 vds-hover:bg-destructive/10" onClick={() => cancelMutation.mutate()} disabled={cancelMutation.isPending}>
                {cancelMutation.isPending
                  ? <><Loader2 className="vds-h-3.5 vds-w-3.5 vds-animate-spin vds-mr-1.5" />{t('jobs.cancelling')}</>
                  : <><X className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />{t('jobs.cancelJob')}</>}
              </Button>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
