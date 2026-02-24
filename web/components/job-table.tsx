'use client'

import { clsx } from 'clsx'
import type { Job } from '@/lib/types'

const STATUS_STYLES: Record<string, string> = {
  completed: 'bg-emerald-900 text-emerald-300 border-emerald-700',
  failed:    'bg-red-900 text-red-300 border-red-700',
  cancelled: 'bg-slate-700 text-slate-300 border-slate-600',
  pending:   'bg-amber-900 text-amber-300 border-amber-700',
  running:   'bg-blue-900 text-blue-300 border-blue-700',
}

function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={clsx(
        'inline-flex items-center px-2 py-0.5 rounded border text-xs font-medium',
        STATUS_STYLES[status] ?? 'bg-slate-700 text-slate-300 border-slate-600',
      )}
    >
      {status}
    </span>
  )
}

function truncateId(id: string) {
  return id.slice(0, 8) + '…'
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

interface JobTableProps {
  jobs: Job[]
}

export default function JobTable({ jobs }: JobTableProps) {
  if (jobs.length === 0) {
    return (
      <div className="rounded-xl border border-slate-800 bg-slate-900 p-8 text-center text-slate-500">
        No jobs found.
      </div>
    )
  }

  return (
    <div className="rounded-xl border border-slate-800 bg-slate-900 overflow-hidden">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-800 bg-slate-900/80">
              <th className="px-4 py-3 text-left font-medium text-slate-400">ID</th>
              <th className="px-4 py-3 text-left font-medium text-slate-400">Model</th>
              <th className="px-4 py-3 text-left font-medium text-slate-400">Backend</th>
              <th className="px-4 py-3 text-left font-medium text-slate-400">Status</th>
              <th className="px-4 py-3 text-left font-medium text-slate-400">Created</th>
              <th className="px-4 py-3 text-right font-medium text-slate-400">Latency</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-800">
            {jobs.map((job) => (
              <tr key={job.id} className="hover:bg-slate-800/50 transition-colors">
                <td className="px-4 py-3 font-mono text-slate-300 text-xs">
                  <span title={job.id}>{truncateId(job.id)}</span>
                </td>
                <td className="px-4 py-3 text-slate-200">{job.model_name}</td>
                <td className="px-4 py-3 text-slate-400">{job.backend}</td>
                <td className="px-4 py-3">
                  <StatusBadge status={job.status} />
                </td>
                <td className="px-4 py-3 text-slate-400 text-xs">
                  {formatDate(job.created_at)}
                </td>
                <td className="px-4 py-3 text-right text-slate-400 tabular-nums">
                  {job.latency_ms != null ? `${job.latency_ms} ms` : '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
