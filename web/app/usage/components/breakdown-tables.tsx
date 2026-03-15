'use client'

import type { ApiKey, ModelBreakdown, UsageBreakdown } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { fmtMs, fmtCompact, fmtCost } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { useTranslation } from '@/i18n'
import { BarChart2 } from 'lucide-react'
import { PROVIDER_BADGE, PROVIDER_COLORS, SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING } from '@/lib/constants'

/* ─── Key breakdown table ─────────────────────────────────── */
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
  if (data.by_key.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )
  const total = data.by_key.reduce((s, k) => s + k.request_count, 0)
  const keyMap = new Map(keys?.map((k) => [k.id, k]) ?? [])

  return (
    <DataTable minWidth="700px">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>{t('usage.keyCol')}</TableHead>
          <TableHead className="text-right w-24">{t('usage.requestsCol')}</TableHead>
          <TableHead className="w-32">{t('usage.shareCol')}</TableHead>
          <TableHead className="text-right w-24">{t('usage.successCol')}</TableHead>
          <TableHead className="text-right w-28">{t('usage.tokensCol')}</TableHead>
          <TableHead className="text-right w-28">{t('usage.estimatedCost')}</TableHead>
          <TableHead className="w-16" />
        </TableRow>
      </TableHeader>
      <TableBody>
        {data.by_key.map((k) => {
          const pct = calcPercentage(k.request_count, total)
          const totalTok = k.prompt_tokens + k.completion_tokens
          const apiKey = keyMap.get(k.key_id)
          const isSelected = selectedKeyId === k.key_id
          return (
            <TableRow
              key={k.key_id}
              className={`cursor-pointer transition-colors ${isSelected ? 'bg-primary/5 hover:bg-primary/8' : ''}`}
              onClick={() => onKeySelect(k.key_id)}
            >
              <TableCell>
                <div className="flex items-center gap-2">
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
                  <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
                    <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${pct}%` }} />
                  </div>
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
                    : <span className="text-foreground">${k.estimated_cost_usd.toFixed(4)}</span>}
              </TableCell>
              <TableCell className="text-right" onClick={(e) => e.stopPropagation()}>
                {apiKey && (
                  <Button variant="ghost" size="icon" className="h-7 w-7 text-muted-foreground hover:text-primary"
                    onClick={() => onKeyClick(apiKey)} title={t('keys.viewUsage')}>
                    <BarChart2 className="h-3.5 w-3.5" />
                  </Button>
                )}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </DataTable>
  )
}

/* ─── Model breakdown table (filterable) ─────────────────── */
export function ModelBreakdownTable({
  data,
  filter,
}: {
  data: ModelBreakdown[]
  filter: string
}) {
  const { t } = useTranslation()
  const filtered = filter.trim()
    ? data.filter((m) => m.model_name.toLowerCase().includes(filter.toLowerCase()))
    : data

  if (filtered.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )

  return (
    <DataTable minWidth="760px">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>{t('usage.modelCol')}</TableHead>
          <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
          <TableHead className="text-right w-24">{t('usage.requestsCol')}</TableHead>
          <TableHead className="w-40">{t('usage.callPct')}</TableHead>
          <TableHead className="text-right w-32">{t('usage.avgLatencyCol')}</TableHead>
          <TableHead className="text-right w-28">{t('usage.tokensCol')}</TableHead>
          <TableHead className="text-right w-28">{t('usage.estimatedCost')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {filtered.map((m, i) => {
          const totalTok = m.prompt_tokens + m.completion_tokens
          const color = PROVIDER_COLORS[m.provider_type] ?? 'var(--theme-primary)'
          return (
            <TableRow key={`${m.model_name}-${m.provider_type}-${i}`}>
              <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
              <TableCell>
                <Badge variant="outline" className={`text-xs ${PROVIDER_BADGE[m.provider_type] ?? ''}`}>
                  {m.provider_type}
                </Badge>
              </TableCell>
              <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(m.request_count)}</TableCell>
              <TableCell>
                <div className="flex items-center gap-2">
                  <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
                    <div className="h-full rounded-full transition-all" style={{ width: `${Math.min(m.call_pct, 100)}%`, background: color }} />
                  </div>
                  <span className="text-xs tabular-nums font-semibold w-10 text-right" style={{ color }}>
                    {m.call_pct.toFixed(1)}%
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
                    : <span className="text-foreground">${m.estimated_cost_usd.toFixed(4)}</span>}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </DataTable>
  )
}
