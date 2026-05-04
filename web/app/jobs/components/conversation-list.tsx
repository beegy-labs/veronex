'use client'

import { useState, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { conversationsQuery, conversationDetailQuery, turnInternalsQuery } from '@/lib/queries'
import type { ConversationTurn, ConversationDetail } from '@/lib/types'
import { useTranslation } from '@/i18n'
import { fmtNumber, fmtDatetime } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { ChevronLeft, ChevronRight, ChevronDown, ChevronUp, MessageSquare, Wrench, Play, RefreshCw, Search, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { StatusPill } from '@/components/status-pill'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { SOURCE_STYLES } from '@/lib/constants'

const PAGE_SIZE = 30

/** Per-turn MCP tool_call shape persisted in S3 ConversationRecord. The
 *  bridge enriches the OpenAI tool_call invocation with execution-side
 *  metadata (`result`, `outcome`, `latency_ms`, `cache_hit`, etc.) before
 *  appending to `turn.tool_calls[]`. PG `mcp_loop_tool_calls` was retired
 *  2026-05-01 in favour of this single S3 source. */
interface McpToolCallInline {
  function?: { name?: string; arguments?: string }
  round?: number
  server_slug?: string
  result?: string
  outcome?: string
  cache_hit?: boolean
  latency_ms?: number
  result_bytes?: number
}

interface ConversationListProps {
  onContinue?: (detail: ConversationDetail) => void
}

export function ConversationList({ onContinue }: ConversationListProps) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [page, setPage] = useState(0)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [sourceFilter, setSourceFilter] = useState('all')

  const { data, isLoading, isFetching, refetch } = useQuery(conversationsQuery({
    page, pageSize: PAGE_SIZE,
    source: sourceFilter !== 'all' ? sourceFilter : undefined,
    search: query || undefined,
  }))
  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0

  const commitSearch = useCallback(() => { setQuery(search); setPage(0) }, [search])
  const clearSearch = useCallback(() => { setSearch(''); setQuery(''); setPage(0) }, [])

  return (
    <div className="vds-space-y-4">
      {/* Controls row — mirrors JobsSection layout */}
      <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-3">
        {data ? (
          <StatusPill icon={<MessageSquare className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={data.total} label={t('jobs.conversations')} />
        ) : (
          <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>
        )}
        <div className="vds-flex vds-items-center vds-gap-2">
          <div className="vds-relative vds-flex vds-items-center">
            <Search className="vds-absolute vds-left-2.5 vds-h-3.5 vds-w-3.5 vds-text-dim vds-pointer-events-none" />
            <Input
              className="vds-pl-8 vds-pr-8 vds-w-44 vds-h-9 vds-text-sm"
              placeholder={t('jobs.searchPlaceholder')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') commitSearch(); if (e.key === 'Escape') clearSearch() }}
            />
            {search && (
              <button type="button" aria-label={t('common.clearSearch')} className="vds-absolute vds-right-2.5 vds-text-dim vds-hover:text-primary" onClick={clearSearch}>
                <X className="vds-h-3.5 vds-w-3.5" />
              </button>
            )}
          </div>
          <Select value={sourceFilter} onValueChange={(v) => { setSourceFilter(v); setPage(0) }}>
            <SelectTrigger className="vds-w-28 vds-h-9 vds-text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('jobs.allSources')}</SelectItem>
              <SelectItem value="api">{t('jobs.sourceApi')}</SelectItem>
              <SelectItem value="test">{t('jobs.sourceTest')}</SelectItem>
              <SelectItem value="analyzer">{t('jobs.sourceAnalyzer')}</SelectItem>
            </SelectContent>
          </Select>
          <Button variant="ghost" size="icon" aria-label={t('common.refresh')} className="vds-h-9 vds-w-9 vds-flex-shrink-0" onClick={() => refetch()} disabled={isFetching}>
            <RefreshCw className={`vds-h-3.5 vds-w-3.5 ${isFetching ? 'vds-animate-spin' : ''}`} />
          </Button>
        </div>
      </div>

      {isLoading && (
        <div className="vds-flex vds-h-48 vds-items-center vds-justify-center vds-text-dim">
          {t('common.loading')}
        </div>
      )}

      {data && data.conversations.length === 0 && (
        <div className="vds-flex vds-h-48 vds-items-center vds-justify-center vds-text-dim">
          {t('jobs.noConversations')}
        </div>
      )}

      {data && data.conversations.length > 0 && (
        <DataTable minWidth="640px">
          <TableHeader>
              <TableRow className="vds-bg-muted/30">
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-left vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('jobs.conversationTitle')}</TableHead>
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-left vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('common.model')}</TableHead>
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-left vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('jobs.source')}</TableHead>
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('jobs.turnCount')}</TableHead>
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('jobs.totalTokens')}</TableHead>
                <TableHead className="vds-px-4 vds-py-2.5 vds-text-right vds-font-500 vds-text-dim vds-whitespace-nowrap">{t('jobs.lastActivity')}</TableHead>
                {onContinue && <TableHead className="vds-px-4 vds-py-2.5" />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.conversations.map((c) => (
                <TableRow
                  key={c.id}
                  className="vds-border-b-1 vds-border-subtle last:border-0 vds-hover:bg-hover/20 vds-cursor-pointer vds-transition-colors"
                  onClick={() => setSelectedId(c.id)}
                >
                  <TableCell className="vds-px-4 vds-py-3">
                    <div className="vds-flex vds-items-center vds-gap-2">
                      <MessageSquare className="vds-h-3.5 vds-w-3.5 vds-text-dim vds-flex-shrink-0" />
                      <span className="vds-font-500 vds-truncate vds-max-w-[300px]">
                        {c.title || c.id}
                      </span>
                    </div>
                  </TableCell>
                  <TableCell className="vds-px-4 vds-py-3 vds-text-dim">{c.model_name || '—'}</TableCell>
                  <TableCell className="vds-px-4 vds-py-3">
                    <span className={`vds-px-1.5 vds-py-0.5 vds-rounded vds-text-[10px] vds-font-mono ${SOURCE_STYLES[c.source] ?? SOURCE_STYLES.api}`}>{c.source}</span>
                  </TableCell>
                  <TableCell className="vds-px-4 vds-py-3 vds-text-right vds-tabular-nums">{c.turn_count}</TableCell>
                  <TableCell className="vds-px-4 vds-py-3 vds-text-right vds-tabular-nums vds-text-dim">
                    {fmtNumber(c.total_prompt_tokens + c.total_completion_tokens)}
                  </TableCell>
                  <TableCell className="vds-px-4 vds-py-3 vds-text-right vds-text-dim vds-text-xs">
                    {fmtDatetime(c.updated_at, tz)}
                  </TableCell>
                  {onContinue && (
                    <TableCell className="vds-px-4 vds-py-3 vds-text-right" onClick={(e) => e.stopPropagation()}>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="vds-h-7 vds-text-xs"
                        onClick={(e) => { e.stopPropagation(); setSelectedId(c.id) }}
                      >
                        <Play className="vds-h-3 vds-w-3 vds-mr-1" />
                        {t('jobs.continue')}
                      </Button>
                    </TableCell>
                  )}
                </TableRow>
              ))}
            </TableBody>
        </DataTable>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="vds-flex vds-items-center vds-justify-end vds-gap-2">
          <Button variant="outline" size="icon" aria-label={t('common.prevPage')} className="vds-h-8 vds-w-8"
            onClick={() => setPage(p => Math.max(0, p - 1))} disabled={page === 0}>
            <ChevronLeft className="vds-h-4 vds-w-4" />
          </Button>
          <span className="vds-text-sm vds-text-dim vds-tabular-nums">
            {page + 1} / {totalPages}
          </span>
          <Button variant="outline" size="icon" aria-label={t('common.nextPage')} className="vds-h-8 vds-w-8"
            onClick={() => setPage(p => Math.min(totalPages - 1, p + 1))} disabled={page >= totalPages - 1}>
            <ChevronRight className="vds-h-4 vds-w-4" />
          </Button>
        </div>
      )}

      {/* Detail modal */}
      {selectedId && (
        <ConversationDetailModal
          id={selectedId}
          onClose={() => setSelectedId(null)}
          onContinue={onContinue ? (detail) => { setSelectedId(null); onContinue(detail) } : undefined}
        />
      )}
    </div>
  )
}

/** Per-turn metadata pills (compression / vision). MCP tool detail moved
 *  inline into the assistant bubble — `turn.tool_calls[]` already carries
 *  `result`, `outcome`, `latency_ms`, `cache_hit`, `server_slug` from S3. */
function TurnInternalsPanel({ convId, jobId }: { convId: string; jobId: string }) {
  const [open, setOpen] = useState(false)
  const { t } = useTranslation()
  const { data, isFetching } = useQuery(turnInternalsQuery(convId, jobId, open))

  const hasMetadata = !!(data?.compressed || data?.vision_analysis)

  return (
    <div className="vds-mt-1.5">
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        className="vds-flex vds-items-center vds-gap-1 vds-text-[10px] vds-text-dim/60 vds-hover:text-dim vds-transition-colors"
      >
        {open ? <ChevronUp className="vds-h-3 vds-w-3" /> : <ChevronDown className="vds-h-3 vds-w-3" />}
        {t('conversations.internals')}
      </button>

      {open && (
        <div className="vds-mt-1 vds-space-y-1.5">
          {isFetching && <span className="vds-text-[10px] vds-text-dim">{t('common.loading')}</span>}
          {data && !hasMetadata && (
            <span className="vds-text-[10px] vds-text-dim/60">{t('conversations.internalsEmpty')}</span>
          )}
          {data?.compressed && (
            <span className="vds-inline-flex vds-items-center vds-gap-1 vds-rounded vds-bg-primary/10 vds-px-1.5 vds-py-0.5 vds-text-[10px] vds-font-mono vds-text-primary">
              {t('conversations.compressedBadge', {
                original: data.compressed.original_tokens,
                compressed: data.compressed.compressed_tokens,
                model: data.compressed.compression_model,
              })}
            </span>
          )}
          {data?.vision_analysis && (
            <span className="vds-inline-flex vds-items-center vds-gap-1 vds-rounded vds-bg-hover/15 vds-px-1.5 vds-py-0.5 vds-text-[10px] vds-font-mono vds-text-primary">
              {t('conversations.visionBadge', {
                model: data.vision_analysis.vision_model,
                imageCount: data.vision_analysis.image_count,
                tokens: data.vision_analysis.analysis_tokens,
              })}
            </span>
          )}
        </div>
      )}
    </div>
  )
}

function ConversationDetailModal({ id, onClose, onContinue }: { id: string; onClose: () => void; onContinue?: (detail: ConversationDetail) => void }) {
  const { t } = useTranslation()
  const { data, isLoading } = useQuery(conversationDetailQuery(id))

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-2xl vds-max-h-[80vh] vds-overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <MessageSquare className="vds-h-4 vds-w-4" />
            {data?.title || id}
          </DialogTitle>
          {data && (() => {
            const totalMcpCalls = data.turns.reduce((acc: number, t: ConversationTurn) => {
              const tcs = (t.tool_calls && Array.isArray(t.tool_calls)) ? t.tool_calls.length : 0
              return acc + tcs
            }, 0)
            return (
            <div className="vds-flex vds-items-center vds-justify-between vds-mt-1">
              <p className="vds-text-xs vds-text-dim">
                <span className={`vds-inline-block vds-px-1.5 vds-py-0.5 vds-rounded vds-font-mono vds-mr-2 ${SOURCE_STYLES[data.source] ?? SOURCE_STYLES.api}`}>{data.source}</span>
                {data.model_name} · {data.turn_count} {t('jobs.turnCount')}
                {totalMcpCalls > 0 && <> · {t('conversations.mcpCallsBadge', { count: totalMcpCalls })}</>}
                {' · '}{fmtNumber(data.total_prompt_tokens + data.total_completion_tokens)} {t('common.tokensUnit')}
              </p>
              {onContinue && (
                <Button type="button" size="sm" variant="outline" className="vds-h-7 vds-text-xs vds-flex-shrink-0" onClick={() => onContinue(data)}>
                  <Play className="vds-h-3 vds-w-3 vds-mr-1" />
                  {t('jobs.continueInTest')}
                </Button>
              )}
            </div>
            )
          })()}
        </DialogHeader>

        {isLoading && <p className="vds-text-dim vds-py-8 vds-text-center">{t('common.loading')}</p>}

        {data && (
          <div className="vds-space-y-3 vds-mt-4">
            {data.turns.map((turn: ConversationTurn, i: number) => (
              <div key={turn.job_id} className="vds-space-y-1">
                {/* User prompt */}
                <div className="vds-rounded-lg vds-bg-primary/10 vds-px-4 vds-py-2.5">
                  <p className="vds-text-[10px] vds-font-600 vds-uppercase vds-text-primary vds-mb-1">{t('jobs.roleUser')}</p>
                  <p className="vds-text-sm vds-whitespace-pre-wrap">{turn.prompt || '—'}</p>
                </div>
                {/* Assistant response */}
                <div className="vds-rounded-lg vds-bg-muted/40 vds-px-4 vds-py-2.5">
                  <div className="vds-flex vds-items-center vds-gap-2 vds-mb-1">
                    <p className="vds-text-[10px] vds-font-600 vds-uppercase vds-text-dim">{t('jobs.roleAssistant')}</p>
                    {turn.model_name && (
                      <span className="vds-text-[10px] vds-font-mono vds-text-dim/60">{turn.model_name}</span>
                    )}
                  </div>
                  {turn.tool_calls && Array.isArray(turn.tool_calls) && turn.tool_calls.length > 0 && (
                    <div className="vds-mb-2 vds-space-y-1">
                      {turn.tool_calls.map((tc: McpToolCallInline, j: number) => (
                        <div key={`tool-${j}-${tc.function?.name ?? ''}`} className="vds-rounded vds-border-1 vds-border-subtle vds-bg-muted/30 vds-px-2 vds-py-1.5">
                          <div className="vds-flex vds-items-center vds-gap-1.5 vds-flex-wrap">
                            <Wrench className="vds-h-3 vds-w-3 vds-text-info vds-flex-shrink-0" />
                            <code className="vds-text-2xs vds-font-mono vds-font-600 vds-text-info">{tc.function?.name ?? 'unknown'}</code>
                            {typeof tc.round === 'number' && <span className="vds-text-[10px] vds-font-mono vds-text-dim/70">round {tc.round}</span>}
                            {tc.outcome && <span className={`vds-text-[10px] vds-font-mono vds-px-1 vds-rounded ${tc.outcome === 'success' || tc.outcome === 'cache_hit' ? 'vds-bg-success/15 vds-text-success' : 'vds-bg-error-bg-fg/15 vds-text-error'}`}>{tc.outcome}</span>}
                            {tc.cache_hit && <span className="vds-text-[10px] vds-font-mono vds-px-1 vds-rounded vds-bg-primary/15 vds-text-primary">cache</span>}
                            {typeof tc.latency_ms === 'number' && <span className="vds-text-[10px] vds-font-mono vds-text-dim/60">{tc.latency_ms}ms</span>}
                          </div>
                          {tc.function?.arguments && (
                            <pre className="vds-text-[10px] vds-font-mono vds-text-primary/60 vds-mt-1 vds-whitespace-pre-wrap vds-break-words vds-max-h-20 vds-overflow-y-auto">
                              {typeof tc.function.arguments === 'string' ? tc.function.arguments : JSON.stringify(tc.function.arguments, null, 2)}
                            </pre>
                          )}
                          {tc.result && (
                            <details className="vds-mt-1">
                              <summary className="vds-text-[10px] vds-text-dim/70 vds-cursor-pointer vds-hover:text-dim">{t('conversations.toolResult')}</summary>
                              <pre className="vds-text-[10px] vds-font-mono vds-text-primary/70 vds-mt-1 vds-whitespace-pre-wrap vds-break-words vds-max-h-40 vds-overflow-y-auto">{tc.result}</pre>
                            </details>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                  {/* Render result body only when meaningful:
                      - has result text → show it (text rounds + S24 synthesis)
                      - empty result + has tool_calls → tool block above is the
                        content; show a small "(tool-only turn)" hint instead of
                        the misleading "(저장된 결과 없음)" that would imply data loss
                      - empty result + no tool_calls → genuinely empty (cancel /
                        error / pre-stream) — show "(저장된 결과 없음)" */}
                  {turn.result ? (
                    <div className="vds-text-sm vds-leading-relaxed vds-break-words vds-space-y-2 [&_p]:my-2 [&_p:first-child]:mt-0 [&_p:last-child]:mb-0">
                      <ReactMarkdown
                        remarkPlugins={[remarkGfm]}
                        components={{
                          h1: ({ children }) => <h1 className="vds-text-base vds-font-700 vds-mt-3 vds-mb-2">{children}</h1>,
                          h2: ({ children }) => <h2 className="vds-text-sm vds-font-700 vds-mt-3 vds-mb-1.5">{children}</h2>,
                          h3: ({ children }) => <h3 className="vds-text-sm vds-font-600 vds-mt-2 vds-mb-1">{children}</h3>,
                          h4: ({ children }) => <h4 className="vds-text-sm vds-font-600 vds-mt-2 vds-mb-1">{children}</h4>,
                          ul: ({ children }) => <ul className="list-disc list-outside vds-ml-5 vds-my-2 vds-space-y-1">{children}</ul>,
                          ol: ({ children }) => <ol className="list-decimal list-outside vds-ml-5 vds-my-2 vds-space-y-1">{children}</ol>,
                          li: ({ children }) => <li className="vds-text-sm">{children}</li>,
                          a: ({ href, children }) => <a href={href} target="_blank" rel="noopener noreferrer" className="vds-text-primary vds-underline vds-hover:no-underline vds-break-all">{children}</a>,
                          code: ({ className, children }) => {
                            const isBlock = /language-/.test(className ?? '')
                            return isBlock
                              ? <code className="vds-block vds-bg-muted/60 vds-px-2 vds-py-1.5 vds-rounded vds-font-mono vds-text-xs vds-my-2 vds-overflow-x-auto vds-whitespace-pre">{children}</code>
                              : <code className="vds-bg-muted/60 vds-px-1 vds-py-0.5 vds-rounded vds-font-mono vds-text-xs">{children}</code>
                          },
                          pre: ({ children }) => <pre className="vds-bg-muted/60 vds-p-2 vds-rounded vds-font-mono vds-text-xs vds-my-2 vds-overflow-x-auto">{children}</pre>,
                          blockquote: ({ children }) => <blockquote className="vds-border-l-2 vds-border-subtle vds-pl-3 vds-italic vds-text-dim vds-my-2">{children}</blockquote>,
                          table: ({ children }) => <table className="vds-border-collapse vds-text-xs vds-my-2">{children}</table>,
                          th: ({ children }) => <th className="vds-border-1 vds-border-subtle vds-px-2 vds-py-1 vds-bg-muted/40 vds-font-600">{children}</th>,
                          td: ({ children }) => <td className="vds-border-1 vds-border-subtle vds-px-2 vds-py-1">{children}</td>,
                          hr: () => <hr className="vds-my-3 vds-border-subtle" />,
                          strong: ({ children }) => <strong className="vds-font-600">{children}</strong>,
                          em: ({ children }) => <em className="vds-italic">{children}</em>,
                        }}
                      >
                        {turn.result}
                      </ReactMarkdown>
                    </div>
                  ) : (turn.tool_calls && Array.isArray(turn.tool_calls) && turn.tool_calls.length > 0) ? (
                    <p className="vds-text-2xs vds-italic vds-text-dim/70">{t('jobs.toolOnlyTurnHint')}</p>
                  ) : (
                    <p className="vds-text-sm vds-whitespace-pre-wrap vds-text-dim/60">({t('jobs.noResult')})</p>
                  )}
                  <TurnInternalsPanel convId={id} jobId={turn.job_id} />
                </div>
                {i < data.turns.length - 1 && <hr className="vds-border-subtle" />}
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
