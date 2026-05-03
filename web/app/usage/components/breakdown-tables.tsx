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
    <div className="vds-py-12 vds-text-center vds-text-dim vds-text-sm">{t('usage.noData')}</div>
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
      className={`${className} vds-cursor-pointer vds-hover:text-primary vds-transition-colors vds-whitespace-nowrap ${sortBy === field ? 'vds-text-primary' : ''}`}
      onClick={() => { setSortBy(field); setPage(0) }}
    >
      {label} {sortBy === field ? '↓' : ''}
    </TableHead>
  )

  return (
    <div className="vds-space-y-2">
      <DataTable minWidth="700px">
        <TableHeader>
          <TableRow className="vds-hover:bg-transparent">
            <TableHead className="vds-whitespace-nowrap">{t('usage.keyCol')}</TableHead>
            {sortHeader('requests', t('usage.requestsCol'), 'vds-text-right vds-w-24')}
            <TableHead className="vds-w-32 vds-whitespace-nowrap">{t('usage.shareCol')}</TableHead>
            <TableHead className="vds-text-right vds-w-24 vds-whitespace-nowrap">{t('usage.successCol')}</TableHead>
            {sortHeader('tokens', t('usage.tokensCol'), 'vds-text-right vds-w-28')}
            {sortHeader('cost', t('usage.estimatedCost'), 'vds-text-right vds-w-28')}
            <TableHead className="vds-w-16" />
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
                className={`vds-cursor-pointer vds-transition-colors ${isSelected ? 'vds-bg-primary/5 vds-hover:bg-primary/8' : ''}`}
                onClick={() => onKeySelect(k.key_id)}
              >
                <TableCell>
                  <div className="vds-flex vds-items-center vds-gap-2">
                    <span className="vds-text-xs vds-text-dim/50 vds-tabular-nums vds-w-5">{rank}</span>
                    {isSelected && <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-primary vds-flex-shrink-0" />}
                    <div>
                      <p className="vds-font-600 vds-text-bright vds-text-sm">{k.key_name}</p>
                      <p className="vds-text-xs vds-text-dim vds-font-mono">{k.key_prefix}…</p>
                    </div>
                  </div>
                </TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-font-600">{fmtCompact(k.request_count)}</TableCell>
                <TableCell>
                  <div className="vds-flex vds-items-center vds-gap-1.5">
                    <ProgressBar pct={pct} className="vds-flex-1" />
                    <span className="vds-text-xs vds-tabular-nums vds-text-dim vds-w-7 vds-text-right">{pct}%</span>
                  </div>
                </TableCell>
                <TableCell className="vds-text-right">
                  <span className={`vds-text-sm vds-font-600 vds-tabular-nums ${k.success_rate >= SUCCESS_RATE_GOOD ? 'vds-text-success' : k.success_rate >= SUCCESS_RATE_WARNING ? 'vds-text-warning' : 'vds-text-error'}`}>
                    {k.success_rate}%
                  </span>
                </TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">{fmtCompact(totalTok)}</TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-text-sm vds-font-mono">
                  {k.estimated_cost_usd == null
                    ? <span className="vds-text-dim">—</span>
                    : k.estimated_cost_usd === 0
                      ? <span className="vds-text-dim">{t('usage.free')}</span>
                      : <span className="vds-text-primary">{fmtCost(k.estimated_cost_usd)}</span>}
                </TableCell>
                <TableCell className="vds-text-right" onClick={(e) => e.stopPropagation()}>
                  {apiKey && (
                    <Button variant="ghost" size="icon" className="vds-h-7 vds-w-7 vds-text-dim vds-hover:text-primary"
                      aria-label={t('keys.viewUsage')} onClick={() => onKeyClick(apiKey)} title={t('keys.viewUsage')}>
                      <BarChart2 className="vds-h-3.5 vds-w-3.5" />
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
      {totalPages > 1 && (
        <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
          <span className="vds-text-xs vds-text-dim vds-tabular-nums vds-mr-2">
            {safePage * KEY_PAGE_SIZE + 1}–{Math.min((safePage + 1) * KEY_PAGE_SIZE, sorted.length)} / {sorted.length}
          </span>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={safePage <= 0}
            onClick={() => setPage(p => p - 1)}>
            <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
          </Button>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={safePage >= totalPages - 1}
            onClick={() => setPage(p => p + 1)}>
            <ChevronRight className="vds-h-3.5 vds-w-3.5" />
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
    <div className="vds-py-12 vds-text-center vds-text-dim vds-text-sm">{t('usage.noData')}</div>
  )

  return (
    <div className="vds-space-y-2">
      <DataTable minWidth="760px">
        <TableHeader>
          <TableRow className="vds-hover:bg-transparent">
            <TableHead className="vds-whitespace-nowrap">{t('usage.modelCol')}</TableHead>
            <TableHead className="vds-w-28 vds-whitespace-nowrap">{t('usage.providerCol')}</TableHead>
            <TableHead className="vds-text-right vds-w-24 vds-whitespace-nowrap">{t('usage.requestsCol')}</TableHead>
            <TableHead className="vds-w-40 vds-whitespace-nowrap">{t('usage.callPct')}</TableHead>
            <TableHead className="vds-text-right vds-w-32 vds-whitespace-nowrap">{t('usage.avgLatencyCol')}</TableHead>
            <TableHead className="vds-text-right vds-w-28 vds-whitespace-nowrap">{t('usage.tokensCol')}</TableHead>
            <TableHead className="vds-text-right vds-w-28 vds-whitespace-nowrap">{t('usage.estimatedCost')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageItems.map((m, i) => {
            const totalTok = m.prompt_tokens + m.completion_tokens
            const color = PROVIDER_COLORS[m.provider_type] ?? tokens.brand.primary
            return (
              <TableRow key={`${m.model_name}-${m.provider_type}-${i}`}>
                <TableCell className="vds-font-mono vds-font-500 vds-text-sm">{m.model_name}</TableCell>
                <TableCell>
                  <Badge variant="outline" className={`vds-text-xs vds-whitespace-nowrap ${PROVIDER_BADGE[m.provider_type] ?? ''}`}>
                    {m.provider_type}
                  </Badge>
                </TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-font-600">{fmtCompact(m.request_count)}</TableCell>
                <TableCell>
                  <div className="vds-flex vds-items-center vds-gap-2">
                    <ProgressBar pct={m.call_pct} colorStyle={color} className="vds-flex-1" />
                    <span className="vds-text-xs vds-tabular-nums vds-font-600 vds-w-10 vds-text-right" style={{ color }}>
                      {fmtPct1(m.call_pct)}
                    </span>
                  </div>
                </TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">
                  {m.avg_latency_ms > 0 ? fmtMs(m.avg_latency_ms) : '—'}
                </TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">{fmtCompact(totalTok)}</TableCell>
                <TableCell className="vds-text-right vds-tabular-nums vds-text-sm vds-font-mono">
                  {m.estimated_cost_usd == null
                    ? <span className="vds-text-dim">—</span>
                    : m.estimated_cost_usd === 0
                      ? <span className="vds-text-dim">{t('usage.free')}</span>
                      : <span className="vds-text-primary">{fmtCost(m.estimated_cost_usd)}</span>}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
      {totalPages > 1 && (
        <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
          <span className="vds-text-xs vds-text-dim vds-tabular-nums vds-mr-2">
            {safePage * MODEL_PAGE_SIZE + 1}–{Math.min((safePage + 1) * MODEL_PAGE_SIZE, filtered.length)} / {filtered.length}
          </span>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={safePage <= 0}
            onClick={() => setModelPage(p => p - 1)}>
            <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
          </Button>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={safePage >= totalPages - 1}
            onClick={() => setModelPage(p => p + 1)}>
            <ChevronRight className="vds-h-3.5 vds-w-3.5" />
          </Button>
        </div>
      )}
    </div>
  )
}
