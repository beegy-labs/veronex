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
        <CardTitle className="vds-text-base vds-flex vds-items-center vds-gap-2">
          <Key className="vds-h-4 vds-w-4 vds-text-primary" />
          {t('performance.byKey')}
        </CardTitle>
        <p className="vds-text-xs vds-text-dim">{t('performance.keyPerformance')}</p>
      </CardHeader>
      <CardContent>
        <DataTable minWidth="640px">
          <TableHeader>
            <TableRow className="vds-hover:bg-transparent">
              <TableHead>{t('performance.keyCol')}</TableHead>
              <TableHead className="vds-text-right vds-w-24">{t('usage.requestsCol')}</TableHead>
              <TableHead className="vds-text-right vds-w-28">{t('usage.successCol')}</TableHead>
              <TableHead className="vds-text-right vds-w-28">{t('usage.tokensCol')}</TableHead>
              <TableHead className="vds-text-right vds-w-28">{t('usage.estimatedCost')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {keys.map((k) => {
              const totalTok = k.prompt_tokens + k.completion_tokens
              return (
                <TableRow key={k.key_id}>
                  <TableCell>
                    <p className="vds-font-600 vds-text-sm">{k.key_name}</p>
                    <p className="vds-text-xs vds-text-dim vds-font-mono">{k.key_prefix}…</p>
                  </TableCell>
                  <TableCell className="vds-text-right vds-tabular-nums vds-font-600">{fmtCompact(k.request_count)}</TableCell>
                  <TableCell className="vds-text-right">
                    <span className={`vds-text-sm vds-font-600 vds-tabular-nums ${
 k.success_rate >= SUCCESS_RATE_GOOD ? 'vds-text-success'
 : k.success_rate >= SUCCESS_RATE_WARNING ? 'vds-text-warning'
 : 'vds-text-error'
 }`}>
                      {k.success_rate}%
                    </span>
                  </TableCell>
                  <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">{fmtCompact(totalTok)}</TableCell>
                  <TableCell className="vds-text-right vds-tabular-nums vds-text-sm vds-font-mono">
                    {k.estimated_cost_usd == null
                      ? <span className="vds-text-dim">—</span>
                      : k.estimated_cost_usd === 0
                        ? <span className="vds-text-dim">{t('usage.free')}</span>
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
