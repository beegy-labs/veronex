'use client'

import type { Job } from '@/lib/types'
import { Badge } from '@/components/ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { Card } from '@/components/ui/card'

const STATUS_VARIANT: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
  completed: 'default',
  failed:    'destructive',
  cancelled: 'secondary',
  pending:   'outline',
  running:   'secondary',
}

const STATUS_EXTRA: Record<string, string> = {
  completed: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30 hover:bg-emerald-500/20',
  failed:    'bg-red-500/15 text-red-400 border-red-500/30 hover:bg-red-500/20',
  cancelled: 'bg-slate-500/15 text-slate-400 border-slate-500/30 hover:bg-slate-500/20',
  pending:   'bg-amber-500/15 text-amber-400 border-amber-500/30 hover:bg-amber-500/20',
  running:   'bg-blue-500/15 text-blue-400 border-blue-500/30 hover:bg-blue-500/20',
}

function StatusBadge({ status }: { status: string }) {
  return (
    <Badge
      variant="outline"
      className={STATUS_EXTRA[status] ?? 'bg-slate-500/15 text-slate-400 border-slate-500/30'}
    >
      {status}
    </Badge>
  )
}

function truncateId(id: string) {
  return id.slice(0, 8) + '…'
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  })
}

export default function JobTable({ jobs }: { jobs: Job[] }) {
  if (jobs.length === 0) {
    return (
      <Card>
        <div className="p-8 text-center text-muted-foreground">No jobs found.</div>
      </Card>
    )
  }

  return (
    <Card>
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>ID</TableHead>
              <TableHead>Model</TableHead>
              <TableHead>Backend</TableHead>
              <TableHead>Status</TableHead>
              <TableHead>Created</TableHead>
              <TableHead className="text-right">Latency</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {jobs.map((job) => (
              <TableRow key={job.id}>
                <TableCell className="font-mono text-xs text-muted-foreground">
                  <span title={job.id}>{truncateId(job.id)}</span>
                </TableCell>
                <TableCell>{job.model_name}</TableCell>
                <TableCell className="text-muted-foreground">{job.backend}</TableCell>
                <TableCell><StatusBadge status={job.status} /></TableCell>
                <TableCell className="text-xs text-muted-foreground">{formatDate(job.created_at)}</TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground">
                  {job.latency_ms != null ? `${job.latency_ms} ms` : '—'}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </Card>
  )
}
