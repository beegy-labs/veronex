'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import JobTable from '@/components/job-table'
import { ChevronLeft, ChevronRight } from 'lucide-react'

const PAGE_SIZE = 50

const STATUS_OPTIONS = [
  { value: '',          label: 'All statuses' },
  { value: 'pending',   label: 'Pending' },
  { value: 'running',   label: 'Running' },
  { value: 'completed', label: 'Completed' },
  { value: 'failed',    label: 'Failed' },
  { value: 'cancelled', label: 'Cancelled' },
]

export default function JobsPage() {
  const [page, setPage] = useState(0)
  const [status, setStatus] = useState('')

  const offset = page * PAGE_SIZE

  const queryKey = ['dashboard-jobs', page, status]
  const { data, isLoading, error } = useQuery({
    queryKey,
    queryFn: () => {
      const params = new URLSearchParams({
        limit: String(PAGE_SIZE),
        offset: String(offset),
      })
      if (status) params.set('status', status)
      return api.jobs(params.toString())
    },
    refetchInterval: 30_000,
  })

  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0

  function handleStatusChange(e: React.ChangeEvent<HTMLSelectElement>) {
    setStatus(e.target.value)
    setPage(0)
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">Jobs</h1>
          <p className="text-slate-400 mt-1 text-sm">
            {data ? `${data.total.toLocaleString()} total` : 'Loading…'}
          </p>
        </div>

        {/* Status filter */}
        <select
          value={status}
          onChange={handleStatusChange}
          className="bg-slate-800 border border-slate-700 text-slate-200 text-sm rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-indigo-500"
        >
          {STATUS_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>

      {isLoading && (
        <div className="flex items-center justify-center h-48 text-slate-400">
          Loading jobs…
        </div>
      )}

      {error && (
        <div className="rounded-xl border border-red-800 bg-red-950 p-6 text-red-300">
          <p className="font-semibold">Failed to load jobs</p>
          <p className="text-sm mt-1 text-red-400">
            {error instanceof Error ? error.message : 'Unknown error'}
          </p>
        </div>
      )}

      {data && <JobTable jobs={data.jobs} />}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between text-sm text-slate-400">
          <span>
            Page {page + 1} of {totalPages}
          </span>
          <div className="flex gap-2">
            <button
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              disabled={page === 0}
              className="p-2 rounded-lg border border-slate-700 hover:bg-slate-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
            <button
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              disabled={page >= totalPages - 1}
              className="p-2 rounded-lg border border-slate-700 hover:bg-slate-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
