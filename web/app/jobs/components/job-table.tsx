'use client'

import { useState } from 'react'
import type { Job, RetryParams } from '@/lib/types'
import { Wrench } from 'lucide-react'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable, DataTableEmpty } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { fmtMsNullable } from '@/lib/chart-theme'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime } from '@/lib/date'
import { JobDetailModal } from './job-detail-modal'
import { STATUS_STYLES, SOURCE_STYLES } from '@/lib/constants'
import { StatusBadge } from './status-badge'

function truncateId(id: string) {
  return id.slice(0, 8) + '…'
}

const formatDuration = fmtMsNullable

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
      <DataTable minWidth="1000px">
        <TableHeader>
          <TableRow>
            <TableHead className="whitespace-nowrap">{t('jobs.id')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.conversationId')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.model')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.provider')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.providerName')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.apiKey')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.endpoint')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.source')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.status')}</TableHead>
            <TableHead className="whitespace-nowrap">{t('jobs.createdAt')}</TableHead>
            <TableHead className="text-right whitespace-nowrap">{t('jobs.ttft')}</TableHead>
            <TableHead className="text-right whitespace-nowrap">{t('jobs.latency')}</TableHead>
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
              <TableCell className="font-mono text-xs text-muted-foreground">
                {job.conversation_id
                  ? <span title={job.conversation_id}>{truncateId(job.conversation_id)}</span>
                  : <span className="opacity-40">—</span>}
              </TableCell>
              <TableCell>{job.model_name}</TableCell>
              <TableCell className="text-muted-foreground capitalize">
                {job.provider_type}
              </TableCell>
              <TableCell className="text-muted-foreground text-sm">
                {job.provider_name ?? <span className="opacity-40">—</span>}
              </TableCell>
              <TableCell className="text-xs text-primary/80">
                {job.source === 'test'
                  ? (job.account_name ?? <span className="text-muted-foreground">—</span>)
                  : (job.api_key_name ?? <span className="text-muted-foreground">—</span>)}
              </TableCell>
              <TableCell className="font-mono text-xs text-muted-foreground max-w-[160px] truncate" title={job.request_path ?? undefined}>
                {job.request_path ?? <span className="opacity-40">—</span>}
              </TableCell>
              <TableCell>
                <span className={`px-1.5 py-0.5 rounded text-[10px] font-mono ${SOURCE_STYLES[job.source] ?? SOURCE_STYLES.api}`}>{job.source}</span>
              </TableCell>
              <TableCell>
                <div className="flex items-center gap-1.5">
                  <StatusBadge status={job.status} />
                  {job.has_tool_calls && (
                    <span title={t('jobs.toolCalls')}>
                      <Wrench className="h-3 w-3 text-status-info-fg shrink-0" />
                    </span>
                  )}
                </div>
              </TableCell>
              <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
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
