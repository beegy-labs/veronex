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
    <div className="space-y-4">
      {/* Controls row — mirrors JobsSection layout */}
      <div className="flex items-center justify-between flex-wrap gap-3">
        {data ? (
          <StatusPill icon={<MessageSquare className="h-3 w-3 shrink-0" />} count={data.total} label={t('jobs.conversations')} />
        ) : (
          <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
        )}
        <div className="flex items-center gap-2">
          <div className="relative flex items-center">
            <Search className="absolute left-2.5 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
            <Input
              className="pl-8 pr-8 w-44 h-9 text-sm"
              placeholder={t('jobs.searchPlaceholder')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') commitSearch(); if (e.key === 'Escape') clearSearch() }}
            />
            {search && (
              <button type="button" aria-label={t('common.clearSearch')} className="absolute right-2.5 text-muted-foreground hover:text-foreground" onClick={clearSearch}>
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
          <Select value={sourceFilter} onValueChange={(v) => { setSourceFilter(v); setPage(0) }}>
            <SelectTrigger className="w-28 h-9 text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('jobs.allSources')}</SelectItem>
              <SelectItem value="api">{t('jobs.sourceApi')}</SelectItem>
              <SelectItem value="test">{t('jobs.sourceTest')}</SelectItem>
              <SelectItem value="analyzer">{t('jobs.sourceAnalyzer')}</SelectItem>
            </SelectContent>
          </Select>
          <Button variant="ghost" size="icon" aria-label={t('common.refresh')} className="h-9 w-9 shrink-0" onClick={() => refetch()} disabled={isFetching}>
            <RefreshCw className={`h-3.5 w-3.5 ${isFetching ? 'animate-spin' : ''}`} />
          </Button>
        </div>
      </div>

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          {t('common.loading')}
        </div>
      )}

      {data && data.conversations.length === 0 && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          {t('jobs.noConversations')}
        </div>
      )}

      {data && data.conversations.length > 0 && (
        <DataTable minWidth="640px">
          <TableHeader>
              <TableRow className="bg-muted/30">
                <TableHead className="px-4 py-2.5 text-left font-medium text-muted-foreground whitespace-nowrap">{t('jobs.conversationTitle')}</TableHead>
                <TableHead className="px-4 py-2.5 text-left font-medium text-muted-foreground whitespace-nowrap">{t('common.model')}</TableHead>
                <TableHead className="px-4 py-2.5 text-left font-medium text-muted-foreground whitespace-nowrap">{t('jobs.source')}</TableHead>
                <TableHead className="px-4 py-2.5 text-right font-medium text-muted-foreground whitespace-nowrap">{t('jobs.turnCount')}</TableHead>
                <TableHead className="px-4 py-2.5 text-right font-medium text-muted-foreground whitespace-nowrap">{t('jobs.totalTokens')}</TableHead>
                <TableHead className="px-4 py-2.5 text-right font-medium text-muted-foreground whitespace-nowrap">{t('jobs.lastActivity')}</TableHead>
                {onContinue && <TableHead className="px-4 py-2.5" />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.conversations.map((c) => (
                <TableRow
                  key={c.id}
                  className="border-b border-border last:border-0 hover:bg-muted/20 cursor-pointer transition-colors"
                  onClick={() => setSelectedId(c.id)}
                >
                  <TableCell className="px-4 py-3">
                    <div className="flex items-center gap-2">
                      <MessageSquare className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                      <span className="font-medium truncate max-w-[300px]">
                        {c.title || c.id}
                      </span>
                    </div>
                  </TableCell>
                  <TableCell className="px-4 py-3 text-muted-foreground">{c.model_name || '—'}</TableCell>
                  <TableCell className="px-4 py-3">
                    <span className={`px-1.5 py-0.5 rounded text-[10px] font-mono ${SOURCE_STYLES[c.source] ?? SOURCE_STYLES.api}`}>{c.source}</span>
                  </TableCell>
                  <TableCell className="px-4 py-3 text-right tabular-nums">{c.turn_count}</TableCell>
                  <TableCell className="px-4 py-3 text-right tabular-nums text-muted-foreground">
                    {fmtNumber(c.total_prompt_tokens + c.total_completion_tokens)}
                  </TableCell>
                  <TableCell className="px-4 py-3 text-right text-muted-foreground text-xs">
                    {fmtDatetime(c.updated_at, tz)}
                  </TableCell>
                  {onContinue && (
                    <TableCell className="px-4 py-3 text-right" onClick={(e) => e.stopPropagation()}>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={(e) => { e.stopPropagation(); setSelectedId(c.id) }}
                      >
                        <Play className="h-3 w-3 mr-1" />
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
        <div className="flex items-center justify-end gap-2">
          <Button variant="outline" size="icon" aria-label={t('common.prevPage')} className="h-8 w-8"
            onClick={() => setPage(p => Math.max(0, p - 1))} disabled={page === 0}>
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <span className="text-sm text-muted-foreground tabular-nums">
            {page + 1} / {totalPages}
          </span>
          <Button variant="outline" size="icon" aria-label={t('common.nextPage')} className="h-8 w-8"
            onClick={() => setPage(p => Math.min(totalPages - 1, p + 1))} disabled={page >= totalPages - 1}>
            <ChevronRight className="h-4 w-4" />
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
    <div className="mt-1.5">
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        className="flex items-center gap-1 text-[10px] text-muted-foreground/60 hover:text-muted-foreground transition-colors"
      >
        {open ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
        {t('conversations.internals')}
      </button>

      {open && (
        <div className="mt-1 space-y-1.5">
          {isFetching && <span className="text-[10px] text-muted-foreground">{t('common.loading')}</span>}
          {data && !hasMetadata && (
            <span className="text-[10px] text-muted-foreground/60">{t('conversations.internalsEmpty')}</span>
          )}
          {data?.compressed && (
            <span className="inline-flex items-center gap-1 rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-mono text-primary">
              {t('conversations.compressedBadge', {
                original: data.compressed.original_tokens,
                compressed: data.compressed.compressed_tokens,
                model: data.compressed.compression_model,
              })}
            </span>
          )}
          {data?.vision_analysis && (
            <span className="inline-flex items-center gap-1 rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-mono text-accent-foreground">
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
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <MessageSquare className="h-4 w-4" />
            {data?.title || id}
          </DialogTitle>
          {data && (() => {
            const totalMcpCalls = data.turns.reduce((acc: number, t: ConversationTurn) => {
              const tcs = (t.tool_calls && Array.isArray(t.tool_calls)) ? t.tool_calls.length : 0
              return acc + tcs
            }, 0)
            return (
            <div className="flex items-center justify-between mt-1">
              <p className="text-xs text-muted-foreground">
                <span className={`inline-block px-1.5 py-0.5 rounded font-mono mr-2 ${SOURCE_STYLES[data.source] ?? SOURCE_STYLES.api}`}>{data.source}</span>
                {data.model_name} · {data.turn_count} {t('jobs.turnCount')}
                {totalMcpCalls > 0 && <> · {t('conversations.mcpCallsBadge', { count: totalMcpCalls })}</>}
                {' · '}{fmtNumber(data.total_prompt_tokens + data.total_completion_tokens)} {t('common.tokensUnit')}
              </p>
              {onContinue && (
                <Button type="button" size="sm" variant="outline" className="h-7 text-xs shrink-0" onClick={() => onContinue(data)}>
                  <Play className="h-3 w-3 mr-1" />
                  {t('jobs.continueInTest')}
                </Button>
              )}
            </div>
            )
          })()}
        </DialogHeader>

        {isLoading && <p className="text-muted-foreground py-8 text-center">{t('common.loading')}</p>}

        {data && (
          <div className="space-y-3 mt-4">
            {data.turns.map((turn: ConversationTurn, i: number) => (
              <div key={turn.job_id} className="space-y-1">
                {/* User prompt */}
                <div className="rounded-lg bg-primary/10 px-4 py-2.5">
                  <p className="text-[10px] font-semibold uppercase text-primary mb-1">{t('jobs.roleUser')}</p>
                  <p className="text-sm whitespace-pre-wrap">{turn.prompt || '—'}</p>
                </div>
                {/* Assistant response */}
                <div className="rounded-lg bg-muted/40 px-4 py-2.5">
                  <div className="flex items-center gap-2 mb-1">
                    <p className="text-[10px] font-semibold uppercase text-muted-foreground">{t('jobs.roleAssistant')}</p>
                    {turn.model_name && (
                      <span className="text-[10px] font-mono text-muted-foreground/60">{turn.model_name}</span>
                    )}
                  </div>
                  {turn.tool_calls && Array.isArray(turn.tool_calls) && turn.tool_calls.length > 0 && (
                    <div className="mb-2 space-y-1">
                      {turn.tool_calls.map((tc: McpToolCallInline, j: number) => (
                        <div key={`tool-${j}-${tc.function?.name ?? ''}`} className="rounded border border-border bg-muted/30 px-2 py-1.5">
                          <div className="flex items-center gap-1.5 flex-wrap">
                            <Wrench className="h-3 w-3 text-status-info-fg shrink-0" />
                            <code className="text-[11px] font-mono font-semibold text-status-info-fg">{tc.function?.name ?? 'unknown'}</code>
                            {typeof tc.round === 'number' && <span className="text-[10px] font-mono text-muted-foreground/70">round {tc.round}</span>}
                            {tc.outcome && <span className={`text-[10px] font-mono px-1 rounded ${tc.outcome === 'success' || tc.outcome === 'cache_hit' ? 'bg-status-ok-fg/15 text-status-ok-fg' : 'bg-status-error-fg/15 text-status-error-fg'}`}>{tc.outcome}</span>}
                            {tc.cache_hit && <span className="text-[10px] font-mono px-1 rounded bg-primary/15 text-primary">cache</span>}
                            {typeof tc.latency_ms === 'number' && <span className="text-[10px] font-mono text-muted-foreground/60">{tc.latency_ms}ms</span>}
                          </div>
                          {tc.function?.arguments && (
                            <pre className="text-[10px] font-mono text-foreground/60 mt-1 whitespace-pre-wrap break-words max-h-20 overflow-y-auto">
                              {typeof tc.function.arguments === 'string' ? tc.function.arguments : JSON.stringify(tc.function.arguments, null, 2)}
                            </pre>
                          )}
                          {tc.result && (
                            <details className="mt-1">
                              <summary className="text-[10px] text-muted-foreground/70 cursor-pointer hover:text-muted-foreground">{t('conversations.toolResult')}</summary>
                              <pre className="text-[10px] font-mono text-foreground/70 mt-1 whitespace-pre-wrap break-words max-h-40 overflow-y-auto">{tc.result}</pre>
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
                    <div className="text-sm leading-relaxed break-words space-y-2 [&_p]:my-2 [&_p:first-child]:mt-0 [&_p:last-child]:mb-0">
                      <ReactMarkdown
                        remarkPlugins={[remarkGfm]}
                        components={{
                          h1: ({ children }) => <h1 className="text-base font-bold mt-3 mb-2">{children}</h1>,
                          h2: ({ children }) => <h2 className="text-sm font-bold mt-3 mb-1.5">{children}</h2>,
                          h3: ({ children }) => <h3 className="text-sm font-semibold mt-2 mb-1">{children}</h3>,
                          h4: ({ children }) => <h4 className="text-sm font-semibold mt-2 mb-1">{children}</h4>,
                          ul: ({ children }) => <ul className="list-disc list-outside ml-5 my-2 space-y-1">{children}</ul>,
                          ol: ({ children }) => <ol className="list-decimal list-outside ml-5 my-2 space-y-1">{children}</ol>,
                          li: ({ children }) => <li className="text-sm">{children}</li>,
                          a: ({ href, children }) => <a href={href} target="_blank" rel="noopener noreferrer" className="text-primary underline hover:no-underline break-all">{children}</a>,
                          code: ({ className, children }) => {
                            const isBlock = /language-/.test(className ?? '')
                            return isBlock
                              ? <code className="block bg-muted/60 px-2 py-1.5 rounded font-mono text-[12px] my-2 overflow-x-auto whitespace-pre">{children}</code>
                              : <code className="bg-muted/60 px-1 py-0.5 rounded font-mono text-[12px]">{children}</code>
                          },
                          pre: ({ children }) => <pre className="bg-muted/60 p-2 rounded font-mono text-[12px] my-2 overflow-x-auto">{children}</pre>,
                          blockquote: ({ children }) => <blockquote className="border-l-2 border-border pl-3 italic text-muted-foreground my-2">{children}</blockquote>,
                          table: ({ children }) => <table className="border-collapse text-[12px] my-2">{children}</table>,
                          th: ({ children }) => <th className="border border-border px-2 py-1 bg-muted/40 font-semibold">{children}</th>,
                          td: ({ children }) => <td className="border border-border px-2 py-1">{children}</td>,
                          hr: () => <hr className="my-3 border-border" />,
                          strong: ({ children }) => <strong className="font-semibold">{children}</strong>,
                          em: ({ children }) => <em className="italic">{children}</em>,
                        }}
                      >
                        {turn.result}
                      </ReactMarkdown>
                    </div>
                  ) : (turn.tool_calls && Array.isArray(turn.tool_calls) && turn.tool_calls.length > 0) ? (
                    <p className="text-[11px] italic text-muted-foreground/70">{t('jobs.toolOnlyTurnHint')}</p>
                  ) : (
                    <p className="text-sm whitespace-pre-wrap text-muted-foreground/60">({t('jobs.noResult')})</p>
                  )}
                  <TurnInternalsPanel convId={id} jobId={turn.job_id} />
                </div>
                {i < data.turns.length - 1 && <hr className="border-border" />}
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
