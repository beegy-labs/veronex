'use client'

import { fmtCompact, fmtCost } from '@/lib/chart-theme'
import { SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING } from '@/lib/constants'
import { Key } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'

interface KeyPerfRow {
  key_id: string
  key_name: string
  key_prefix: string
  request_count: number
  success_rate: number
  prompt_tokens: number
  completion_tokens: number
  estimated_cost_usd: number | null
}

export function KeyPerformanceSection({ keys }: { keys: KeyPerfRow[] }) {
  const { t } = useTranslation()
  if (keys.length === 0) return null

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base flex items-center gap-2">
          <Key className="h-4 w-4 text-primary" />
          {t('performance.byKey')}
        </CardTitle>
        <p className="text-xs text-muted-foreground">{t('performance.keyPerformance')}</p>
      </CardHeader>
      <CardContent>
        <DataTable minWidth="640px">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>{t('performance.keyCol')}</TableHead>
              <TableHead className="text-right w-24">{t('usage.requestsCol')}</TableHead>
              <TableHead className="text-right w-28">{t('usage.successCol')}</TableHead>
              <TableHead className="text-right w-28">{t('usage.tokensCol')}</TableHead>
              <TableHead className="text-right w-28">{t('usage.estimatedCost')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {keys.map((k) => {
              const totalTok = k.prompt_tokens + k.completion_tokens
              return (
                <TableRow key={k.key_id}>
                  <TableCell>
                    <p className="font-semibold text-sm">{k.key_name}</p>
                    <p className="text-xs text-muted-foreground font-mono">{k.key_prefix}…</p>
                  </TableCell>
                  <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(k.request_count)}</TableCell>
                  <TableCell className="text-right">
                    <span className={`text-sm font-semibold tabular-nums ${
                      k.success_rate >= SUCCESS_RATE_GOOD ? 'text-status-success-fg'
                        : k.success_rate >= SUCCESS_RATE_WARNING ? 'text-status-warning-fg'
                        : 'text-status-error-fg'
                    }`}>
                      {k.success_rate}%
                    </span>
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmtCompact(totalTok)}</TableCell>
                  <TableCell className="text-right tabular-nums text-sm font-mono">
                    {k.estimated_cost_usd == null
                      ? <span className="text-muted-foreground">—</span>
                      : k.estimated_cost_usd === 0
                        ? <span className="text-muted-foreground">{t('usage.free')}</span>
                        : <span>{fmtCost(k.estimated_cost_usd)}</span>}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </DataTable>
      </CardContent>
    </Card>
  )
}
