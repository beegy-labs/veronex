'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { conversationsQuery, conversationDetailQuery } from '@/lib/queries'
import type { ConversationTurn, ConversationDetail } from '@/lib/types'
import { useTranslation } from '@/i18n'
import { fmtNumber, fmtDatetime } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { ChevronLeft, ChevronRight, MessageSquare, Wrench, Play } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { StatusPill } from '@/components/status-pill'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'

const PAGE_SIZE = 30

interface ConversationListProps {
  onContinue?: (detail: ConversationDetail) => void
}

export function ConversationList({ onContinue }: ConversationListProps) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [page, setPage] = useState(0)
  const [selectedId, setSelectedId] = useState<string | null>(null)

  const { data, isLoading } = useQuery(conversationsQuery({ page, pageSize: PAGE_SIZE }))
  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0

  return (
    <div className="space-y-4">
      {data && (
        <StatusPill
          icon={<MessageSquare className="h-3 w-3 shrink-0" />}
          count={data.total}
          label={t('jobs.conversations')}
        />
      )}

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
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/30">
                <th className="px-4 py-2.5 text-left font-medium text-muted-foreground">{t('jobs.conversationTitle')}</th>
                <th className="px-4 py-2.5 text-left font-medium text-muted-foreground">{t('common.model')}</th>
                <th className="px-4 py-2.5 text-left font-medium text-muted-foreground">{t('jobs.source')}</th>
                <th className="px-4 py-2.5 text-right font-medium text-muted-foreground">{t('jobs.turnCount')}</th>
                <th className="px-4 py-2.5 text-right font-medium text-muted-foreground">{t('jobs.totalTokens')}</th>
                <th className="px-4 py-2.5 text-right font-medium text-muted-foreground">{t('jobs.lastActivity')}</th>
                {onContinue && <th className="px-4 py-2.5" />}
              </tr>
            </thead>
            <tbody>
              {data.conversations.map((c) => (
                <tr
                  key={c.id}
                  className="border-b border-border last:border-0 hover:bg-muted/20 cursor-pointer transition-colors"
                  onClick={() => setSelectedId(c.public_id)}
                >
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2">
                      <MessageSquare className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                      <span className="font-medium truncate max-w-[300px]">
                        {c.title || c.public_id}
                      </span>
                    </div>
                  </td>
                  <td className="px-4 py-3 text-muted-foreground">{c.model_name || '—'}</td>
                  <td className="px-4 py-3">
                    <span className={`px-1.5 py-0.5 rounded text-[10px] font-mono ${
                      c.source === 'test' ? 'bg-status-warning/15 text-status-warning-fg' :
                      c.source === 'analyzer' ? 'bg-accent/15 text-accent-foreground' :
                      'bg-primary/10 text-primary'
                    }`}>{c.source}</span>
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums">{c.turn_count}</td>
                  <td className="px-4 py-3 text-right tabular-nums text-muted-foreground">
                    {fmtNumber(c.total_prompt_tokens + c.total_completion_tokens)}
                  </td>
                  <td className="px-4 py-3 text-right text-muted-foreground text-xs">
                    {fmtDatetime(c.updated_at, tz)}
                  </td>
                  {onContinue && (
                    <td className="px-4 py-3 text-right" onClick={(e) => e.stopPropagation()}>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={(e) => { e.stopPropagation(); setSelectedId(c.public_id) }}
                      >
                        <Play className="h-3 w-3 mr-1" />
                        {t('jobs.continue')}
                      </Button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-end gap-2">
          <Button variant="outline" size="icon" className="h-8 w-8"
            onClick={() => setPage(p => Math.max(0, p - 1))} disabled={page === 0}>
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <span className="text-sm text-muted-foreground tabular-nums">
            {page + 1} / {totalPages}
          </span>
          <Button variant="outline" size="icon" className="h-8 w-8"
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
          {data && (
            <div className="flex items-center justify-between mt-1">
              <p className="text-xs text-muted-foreground">
                <span className={`inline-block px-1.5 py-0.5 rounded font-mono mr-2 ${
                  data.source === 'test' ? 'bg-status-warning/15 text-status-warning-fg' :
                  data.source === 'analyzer' ? 'bg-accent/15 text-accent-foreground' :
                  'bg-primary/10 text-primary'
                }`}>{data.source}</span>
                {data.model_name} · {data.turn_count} {t('jobs.turnCount')} · {fmtNumber(data.total_prompt_tokens + data.total_completion_tokens)} tokens
              </p>
              {onContinue && (
                <Button type="button" size="sm" variant="outline" className="h-7 text-xs shrink-0" onClick={() => onContinue(data)}>
                  <Play className="h-3 w-3 mr-1" />
                  {t('jobs.continueInTest')}
                </Button>
              )}
            </div>
          )}
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
                      {turn.tool_calls.map((tc: { function?: { name?: string; arguments?: string } }, j: number) => (
                        <div key={j} className="rounded border border-border bg-muted/30 px-2 py-1.5">
                          <div className="flex items-center gap-1.5">
                            <Wrench className="h-3 w-3 text-status-info-fg shrink-0" />
                            <code className="text-[11px] font-mono font-semibold text-status-info-fg">{tc.function?.name ?? 'unknown'}</code>
                          </div>
                          {tc.function?.arguments && (
                            <pre className="text-[10px] font-mono text-foreground/60 mt-1 whitespace-pre-wrap break-words max-h-20 overflow-y-auto">
                              {typeof tc.function.arguments === 'string' ? tc.function.arguments : JSON.stringify(tc.function.arguments, null, 2)}
                            </pre>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                  <p className="text-sm whitespace-pre-wrap">{turn.result || `(${t('jobs.noResult')})`}</p>
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
