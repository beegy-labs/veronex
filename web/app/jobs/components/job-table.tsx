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
            <TableHead className="vds-whitespace-nowrap">{t('jobs.id')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.conversationId')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.model')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.provider')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.providerName')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.apiKey')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.endpoint')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.source')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.status')}</TableHead>
            <TableHead className="vds-whitespace-nowrap">{t('jobs.createdAt')}</TableHead>
            <TableHead className="vds-text-right vds-whitespace-nowrap">{t('jobs.ttft')}</TableHead>
            <TableHead className="vds-text-right vds-whitespace-nowrap">{t('jobs.latency')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {jobs.map((job) => (
            <TableRow
              key={job.id}
              className="vds-cursor-pointer vds-hover:bg-hover/50"
              onClick={() => setSelectedId(job.id)}
            >
              <TableCell className="vds-font-mono vds-text-xs vds-text-dim">
                <span title={job.id}>{truncateId(job.id)}</span>
              </TableCell>
              <TableCell className="vds-font-mono vds-text-xs vds-text-dim">
                {job.conversation_id
                  ? <span title={job.conversation_id}>{truncateId(job.conversation_id)}</span>
                  : <span className="vds-opacity-40">—</span>}
              </TableCell>
              <TableCell>{job.model_name}</TableCell>
              <TableCell className="vds-text-dim vds-capitalize">
                {job.provider_type}
              </TableCell>
              <TableCell className="vds-text-dim vds-text-sm">
                {job.provider_name ?? <span className="vds-opacity-40">—</span>}
              </TableCell>
              <TableCell className="vds-text-xs vds-text-primary/80">
                {job.source === 'test'
                  ? (job.account_name ?? <span className="vds-text-dim">—</span>)
                  : (job.api_key_name ?? <span className="vds-text-dim">—</span>)}
              </TableCell>
              <TableCell className="vds-font-mono vds-text-xs vds-text-dim vds-max-w-[160px] vds-truncate" title={job.request_path ?? undefined}>
                {job.request_path ?? <span className="vds-opacity-40">—</span>}
              </TableCell>
              <TableCell>
                <span className={`vds-px-1.5 vds-py-0.5 vds-rounded vds-text-[10px] vds-font-mono ${SOURCE_STYLES[job.source] ?? SOURCE_STYLES.api}`}>{job.source}</span>
              </TableCell>
              <TableCell>
                <div className="vds-flex vds-items-center vds-gap-1.5">
                  <StatusBadge status={job.status} />
                  {job.has_tool_calls && (
                    <span title={t('jobs.toolCalls')}>
                      <Wrench className="vds-h-3 vds-w-3 vds-text-info vds-flex-shrink-0" />
                    </span>
                  )}
                </div>
              </TableCell>
              <TableCell className="vds-text-xs vds-text-dim vds-whitespace-nowrap">
                {fmtDatetime(job.created_at, tz)}
              </TableCell>
              <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-xs">
                {formatDuration(job.ttft_ms)}
              </TableCell>
              <TableCell className="vds-text-right vds-tabular-nums vds-text-dim">
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
