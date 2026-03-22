'use client'

import { useState, useMemo } from 'react'
import type { ApiKey, ModelBreakdown, UsageBreakdown } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { fmtMs, fmtCompact, fmtCost, fmtPct1 } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { useTranslation } from '@/i18n'
import { BarChart2 } from 'lucide-react'
import { PROVIDER_BADGE, PROVIDER_COLORS, SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING } from '@/lib/constants'
import { tokens } from '@/lib/design-tokens'
import { ProgressBar } from '@/components/progress-bar'

const KEY_PAGE_SIZE = 10

/* ─── Key breakdown table ─────────────────────────────────── */
type SortField = 'tokens' | 'requests' | 'cost'

export function KeyBreakdownTable({
  data,
  keys,
  selectedKeyId,
  onKeyClick,
  onKeySelect,
}: {
  data: UsageBreakdown
  keys: ApiKey[] | undefined
  selectedKeyId: string | null
  onKeyClick: (key: ApiKey) => void
  onKeySelect: (id: string) => void
}) {
  const { t } = useTranslation()
  const [page, setPage] = useState(0)
  const [sortBy, setSortBy] = useState<SortField>('tokens')

  if (data.by_key.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )

  const sorted = useMemo(() => {
    const items = [...data.by_key]
    items.sort((a, b) => {
      if (sortBy === 'tokens') return (b.prompt_tokens + b.completion_tokens) - (a.prompt_tokens + a.completion_tokens)
      if (sortBy === 'requests') return b.request_count - a.request_count
      return (b.estimated_cost_usd ?? 0) - (a.estimated_cost_usd ?? 0)
    })
    return items
  }, [data.by_key, sortBy])

  const totalPages = Math.max(1, Math.ceil(sorted.length / KEY_PAGE_SIZE))
  const safePage = Math.min(page, totalPages - 1)
  const pageItems = sorted.slice(safePage * KEY_PAGE_SIZE, (safePage + 1) * KEY_PAGE_SIZE)
  const total = data.by_key.reduce((s, k) => s + k.request_count, 0)
  const keyMap = new Map(keys?.map((k) => [k.id, k]) ?? [])

  const sortHeader = (field: SortField, label: string, className: string) => (
    <TableHead
      className={`${className} cursor-pointer hover:text-foreground transition-colors whitespace-nowrap ${sortBy === field ? 'text-primary' : ''}`}
      onClick={() => { setSortBy(field); setPage(0) }}
    >
      {label} {sortBy === field ? '↓' : ''}
    </TableHead>
  )

  return (
    <div className="space-y-2">
      <DataTable minWidth="700px">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="whitespace-nowrap">{t('usage.keyCol')}</TableHead>
            {sortHeader('requests', t('usage.requestsCol'), 'text-right w-24')}
            <TableHead className="w-32 whitespace-nowrap">{t('usage.shareCol')}</TableHead>
            <TableHead className="text-right w-24 whitespace-nowrap">{t('usage.successCol')}</TableHead>
            {sortHeader('tokens', t('usage.tokensCol'), 'text-right w-28')}
            {sortHeader('cost', t('usage.estimatedCost'), 'text-right w-28')}
            <TableHead className="w-16" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageItems.map((k, idx) => {
            const pct = calcPercentage(k.request_count, total)
            const totalTok = k.prompt_tokens + k.completion_tokens
            const apiKey = keyMap.get(k.key_id)
            const isSelected = selectedKeyId === k.key_id
            const rank = safePage * KEY_PAGE_SIZE + idx + 1
            return (
              <TableRow
                key={k.key_id}
                className={`cursor-pointer transition-colors ${isSelected ? 'bg-primary/5 hover:bg-primary/8' : ''}`}
                onClick={() => onKeySelect(k.key_id)}
              >
                <TableCell>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground/50 tabular-nums w-5">{rank}</span>
                    {isSelected && <span className="h-1.5 w-1.5 rounded-full bg-primary shrink-0" />}
                    <div>
                      <p className="font-semibold text-text-bright text-sm">{k.key_name}</p>
                      <p className="text-xs text-muted-foreground font-mono">{k.key_prefix}…</p>
                    </div>
                  </div>
                </TableCell>
                <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(k.request_count)}</TableCell>
                <TableCell>
                  <div className="flex items-center gap-1.5">
                    <ProgressBar pct={pct} className="flex-1" />
                    <span className="text-xs tabular-nums text-muted-foreground w-7 text-right">{pct}%</span>
                  </div>
                </TableCell>
                <TableCell className="text-right">
                  <span className={`text-sm font-semibold tabular-nums ${k.success_rate >= SUCCESS_RATE_GOOD ? 'text-status-success-fg' : k.success_rate >= SUCCESS_RATE_WARNING ? 'text-status-warning-fg' : 'text-status-error-fg'}`}>
                    {k.success_rate}%
                  </span>
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmtCompact(totalTok)}</TableCell>
                <TableCell className="text-right tabular-nums text-sm font-mono">
                  {k.estimated_cost_usd == null
                    ? <span className="text-muted-foreground">—</span>
                    : k.estimated_cost_usd === 0
                      ? <span className="text-muted-foreground">{t('usage.free')}</span>
                      : <span className="text-foreground">{fmtCost(k.estimated_cost_usd)}</span>}
                </TableCell>
                <TableCell className="text-right" onClick={(e) => e.stopPropagation()}>
                  {apiKey && (
                    <Button variant="ghost" size="icon" className="h-7 w-7 text-muted-foreground hover:text-primary"
                      aria-label={t('keys.viewUsage')} onClick={() => onKeyClick(apiKey)} title={t('keys.viewUsage')}>
                      <BarChart2 className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
      {totalPages > 1 && (
        <div className="flex items-center justify-end gap-1">
          <span className="text-xs text-muted-foreground tabular-nums mr-2">
            {safePage * KEY_PAGE_SIZE + 1}–{Math.min((safePage + 1) * KEY_PAGE_SIZE, sorted.length)} / {sorted.length}
          </span>
          <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage <= 0}
            onClick={() => setPage(p => p - 1)}>
            <ChevronLeft className="h-3.5 w-3.5" />
          </Button>
          <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage >= totalPages - 1}
            onClick={() => setPage(p => p + 1)}>
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}
    </div>
  )
}

/* ─── Model breakdown table (filterable + paginated) ──────── */
const MODEL_PAGE_SIZE = 10

export function ModelBreakdownTable({
  data,
  filter,
}: {
  data: ModelBreakdown[]
  filter: string
}) {
  const { t } = useTranslation()
  const [modelPage, setModelPage] = useState(0)

  const filtered = useMemo(() => {
    const list = filter.trim()
      ? data.filter((m) => m.model_name.toLowerCase().includes(filter.toLowerCase()))
      : data
    return [...list].sort((a, b) => b.request_count - a.request_count)
  }, [data, filter])

  const totalPages = Math.max(1, Math.ceil(filtered.length / MODEL_PAGE_SIZE))
  const safePage = Math.min(modelPage, totalPages - 1)
  const pageItems = filtered.slice(safePage * MODEL_PAGE_SIZE, (safePage + 1) * MODEL_PAGE_SIZE)

  if (filtered.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )

  return (
    <div className="space-y-2">
      <DataTable minWidth="760px">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="whitespace-nowrap">{t('usage.modelCol')}</TableHead>
            <TableHead className="w-28 whitespace-nowrap">{t('usage.providerCol')}</TableHead>
            <TableHead className="text-right w-24 whitespace-nowrap">{t('usage.requestsCol')}</TableHead>
            <TableHead className="w-40 whitespace-nowrap">{t('usage.callPct')}</TableHead>
            <TableHead className="text-right w-32 whitespace-nowrap">{t('usage.avgLatencyCol')}</TableHead>
            <TableHead className="text-right w-28 whitespace-nowrap">{t('usage.tokensCol')}</TableHead>
            <TableHead className="text-right w-28 whitespace-nowrap">{t('usage.estimatedCost')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageItems.map((m, i) => {
            const totalTok = m.prompt_tokens + m.completion_tokens
            const color = PROVIDER_COLORS[m.provider_type] ?? tokens.brand.primary
            return (
              <TableRow key={`${m.model_name}-${m.provider_type}-${i}`}>
                <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                <TableCell>
                  <Badge variant="outline" className={`text-xs whitespace-nowrap ${PROVIDER_BADGE[m.provider_type] ?? ''}`}>
                    {m.provider_type}
                  </Badge>
                </TableCell>
                <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(m.request_count)}</TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <ProgressBar pct={m.call_pct} colorStyle={color} className="flex-1" />
                    <span className="text-xs tabular-nums font-semibold w-10 text-right" style={{ color }}>
                      {fmtPct1(m.call_pct)}
                    </span>
                  </div>
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                  {m.avg_latency_ms > 0 ? fmtMs(m.avg_latency_ms) : '—'}
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmtCompact(totalTok)}</TableCell>
                <TableCell className="text-right tabular-nums text-sm font-mono">
                  {m.estimated_cost_usd == null
                    ? <span className="text-muted-foreground">—</span>
                    : m.estimated_cost_usd === 0
                      ? <span className="text-muted-foreground">{t('usage.free')}</span>
                      : <span className="text-foreground">{fmtCost(m.estimated_cost_usd)}</span>}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
      {totalPages > 1 && (
        <div className="flex items-center justify-end gap-1">
          <span className="text-xs text-muted-foreground tabular-nums mr-2">
            {safePage * MODEL_PAGE_SIZE + 1}–{Math.min((safePage + 1) * MODEL_PAGE_SIZE, filtered.length)} / {filtered.length}
          </span>
          <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage <= 0}
            onClick={() => setModelPage(p => p - 1)}>
            <ChevronLeft className="h-3.5 w-3.5" />
          </Button>
          <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage >= totalPages - 1}
            onClick={() => setModelPage(p => p + 1)}>
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}
    </div>
  )
}
