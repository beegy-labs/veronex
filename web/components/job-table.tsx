'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import type { Job, JobDetail } from '@/lib/types'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Card } from '@/components/ui/card'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'

// ── Status styling ─────────────────────────────────────────────────────────────

const STATUS_EXTRA: Record<string, string> = {
  completed: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30',
  failed:    'bg-red-500/15 text-red-400 border-red-500/30',
  cancelled: 'bg-slate-500/15 text-slate-400 border-slate-500/30',
  pending:   'bg-amber-500/15 text-amber-400 border-amber-500/30',
  running:   'bg-blue-500/15 text-blue-400 border-blue-500/30',
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

// ── Formatters ─────────────────────────────────────────────────────────────────

function truncateId(id: string) {
  return id.slice(0, 8) + '…'
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  })
}

/** Smart duration: 842ms → "842ms", 2340ms → "2.3s", 125000ms → "2m 5s" */
function formatDuration(ms: number | null): string {
  if (ms == null) return '—'
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const m = Math.floor(ms / 60_000)
  const s = Math.round((ms % 60_000) / 1000)
  return s > 0 ? `${m}m ${s}s` : `${m}m`
}

// ── Job detail modal ───────────────────────────────────────────────────────────

function JobDetailModal({
  jobId,
  open,
  onClose,
}: {
  jobId: string | null
  open: boolean
  onClose: () => void
}) {
  const { data, isLoading } = useQuery<JobDetail>({
    queryKey: ['job-detail', jobId],
    queryFn: () => api.jobDetail(jobId!),
    enabled: !!jobId && open,
    staleTime: 30_000,
  })

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) onClose() }}>
      <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col gap-0 p-0 overflow-hidden">
        <DialogHeader className="px-6 pt-5 pb-4 border-b border-border shrink-0">
          <DialogTitle className="flex items-center gap-3 flex-wrap">
            {data ? (
              <>
                <span className="font-mono text-xs text-muted-foreground">{data.id}</span>
                <StatusBadge status={data.status} />
                <span className="text-sm font-normal text-muted-foreground">
                  {data.model_name} · {data.backend}
                </span>
              </>
            ) : (
              <span className="text-muted-foreground text-sm">Loading…</span>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="overflow-y-auto flex-1">
          {isLoading && (
            <div className="p-6 text-center text-muted-foreground text-sm">Loading…</div>
          )}

          {data && (
            <div className="flex flex-col gap-0 divide-y divide-border">
              {/* Timing row */}
              <div className="px-6 py-3 grid grid-cols-3 gap-x-4 gap-y-1 text-xs">
                <MetaItem label="Created"   value={formatDate(data.created_at)} />
                <MetaItem label="Started"   value={data.started_at   ? formatDate(data.started_at)   : '—'} />
                <MetaItem label="Completed" value={data.completed_at ? formatDate(data.completed_at) : '—'} />
                <MetaItem label="Latency"   value={formatDuration(data.latency_ms)} />
                <MetaItem label="TTFT"      value={formatDuration(data.ttft_ms)} />
                <MetaItem
                  label="TPS"
                  value={data.tps != null ? `${data.tps.toFixed(1)} tok/s` : '—'}
                />
                <MetaItem
                  label="Tokens"
                  value={data.completion_tokens != null ? data.completion_tokens.toLocaleString() : '—'}
                />
                {data.api_key_name && (
                  <MetaItem label="API Key" value={data.api_key_name} accent />
                )}
              </div>

              {/* Prompt */}
              <TextSection
                label="Prompt"
                text={data.prompt || '(empty)'}
                labelClass="text-indigo-400"
              />

              {/* Result or error */}
              {data.status === 'failed' ? (
                <TextSection
                  label="Error"
                  text={data.error || '(no error message)'}
                  labelClass="text-red-400"
                  textClass="text-red-300/80"
                />
              ) : (
                <TextSection
                  label="Result"
                  text={data.result_text || (
                    data.status === 'completed'
                      ? '(no result stored)'
                      : data.status === 'running'
                        ? '(processing…)'
                        : '(pending)'
                  )}
                  labelClass="text-emerald-400"
                />
              )}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}

function MetaItem({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div>
      <span className="text-muted-foreground">{label}: </span>
      <span className={`tabular-nums ${accent ? 'text-primary' : 'text-foreground'}`}>{value}</span>
    </div>
  )
}

function TextSection({
  label,
  text,
  labelClass = '',
  textClass = '',
}: {
  label: string
  text: string
  labelClass?: string
  textClass?: string
}) {
  return (
    <div className="px-6 py-4">
      <p className={`text-xs font-semibold tracking-wider uppercase mb-2 ${labelClass}`}>
        {label}
      </p>
      <pre
        className={`text-sm font-mono whitespace-pre-wrap break-words leading-relaxed text-foreground/85 max-h-52 overflow-y-auto ${textClass}`}
      >
        {text}
      </pre>
    </div>
  )
}

// ── Job table ──────────────────────────────────────────────────────────────────

export default function JobTable({ jobs }: { jobs: Job[] }) {
  const [selectedId, setSelectedId] = useState<string | null>(null)

  if (jobs.length === 0) {
    return (
      <Card>
        <div className="p-8 text-center text-muted-foreground">No jobs found.</div>
      </Card>
    )
  }

  return (
    <>
      <Card>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>ID</TableHead>
                <TableHead>Model</TableHead>
                <TableHead>Backend</TableHead>
                <TableHead>API Key</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="text-right">TTFT</TableHead>
                <TableHead className="text-right">Latency</TableHead>
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
                  <TableCell>{job.model_name}</TableCell>
                  <TableCell className="text-muted-foreground">{job.backend}</TableCell>
                  <TableCell className="text-xs text-primary/80">
                    {job.api_key_name ?? <span className="text-muted-foreground">—</span>}
                  </TableCell>
                  <TableCell><StatusBadge status={job.status} /></TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatDate(job.created_at)}
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
          </Table>
        </div>
      </Card>

      <JobDetailModal
        jobId={selectedId}
        open={!!selectedId}
        onClose={() => setSelectedId(null)}
      />
    </>
  )
}
