'use client'

import { useState, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import JobTable from '@/components/job-table'
import { ChevronLeft, ChevronRight, Search, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

const PAGE_SIZE = 50

const STATUS_OPTIONS = [
  { value: 'all',       label: 'All statuses' },
  { value: 'pending',   label: 'Pending' },
  { value: 'running',   label: 'Running' },
  { value: 'completed', label: 'Completed' },
  { value: 'failed',    label: 'Failed' },
  { value: 'cancelled', label: 'Cancelled' },
]

// ── Pagination helpers ─────────────────────────────────────────────────────────

/** Returns an array of page numbers and '…' ellipsis markers to render. */
function buildPageSlots(current: number, total: number): (number | '…')[] {
  if (total <= 7) return Array.from({ length: total }, (_, i) => i)

  const first = 0
  const last  = total - 1

  if (current <= 3) {
    return [0, 1, 2, 3, 4, '…', last]
  }
  if (current >= total - 4) {
    return [first, '…', last - 4, last - 3, last - 2, last - 1, last]
  }
  return [first, '…', current - 1, current, current + 1, '…', last]
}

// ── Page component ─────────────────────────────────────────────────────────────

export default function JobsPage() {
  const [page, setPage]     = useState(0)
  const [status, setStatus] = useState('all')
  const [search, setSearch] = useState('')
  const [query, setQuery]   = useState('')   // committed search (on Enter)

  const offset = page * PAGE_SIZE

  const { data, isLoading, error } = useQuery({
    queryKey: ['dashboard-jobs', page, status, query],
    queryFn: () => {
      const params = new URLSearchParams({
        limit:  String(PAGE_SIZE),
        offset: String(offset),
      })
      if (status !== 'all') params.set('status', status)
      if (query.trim())     params.set('q', query.trim())
      return api.jobs(params.toString())
    },
    refetchInterval: 30_000,
  })

  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0
  const firstItem  = data && data.total > 0 ? offset + 1 : 0
  const lastItem   = data ? Math.min(offset + PAGE_SIZE, data.total) : 0

  const commitSearch = useCallback(() => { setQuery(search); setPage(0) }, [search])
  const clearSearch  = useCallback(() => { setSearch(''); setQuery(''); setPage(0) }, [])

  const goTo = (p: number) => setPage(Math.max(0, Math.min(totalPages - 1, p)))

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">Jobs</h1>
          <p className="text-slate-400 mt-1 text-sm">
            {data ? `${data.total.toLocaleString()} total` : 'Loading…'}
          </p>
        </div>

        <div className="flex items-center gap-2 flex-wrap">
          {/* Prompt search */}
          <div className="relative flex items-center">
            <Search className="absolute left-2.5 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
            <Input
              className="pl-8 pr-8 w-56 h-9 text-sm"
              placeholder="Search prompt…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitSearch()
                if (e.key === 'Escape') clearSearch()
              }}
            />
            {search && (
              <button
                className="absolute right-2.5 text-muted-foreground hover:text-foreground"
                onClick={clearSearch}
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>

          {/* Status filter */}
          <Select value={status} onValueChange={(val) => { setStatus(val); setPage(0) }}>
            <SelectTrigger className="w-40 h-9">
              <SelectValue placeholder="All statuses" />
            </SelectTrigger>
            <SelectContent>
              {STATUS_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Active search badge */}
      {query && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span>Searching for</span>
          <span className="px-2 py-0.5 rounded bg-primary/15 text-primary font-mono text-xs">
            {query}
          </span>
          <button className="underline text-xs hover:text-foreground" onClick={clearSearch}>
            clear
          </button>
        </div>
      )}

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          Loading jobs…
        </div>
      )}

      {error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-6">
            <p className="font-semibold text-destructive">Failed to load jobs</p>
            <p className="text-sm mt-1 text-destructive/80">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </CardContent>
        </Card>
      )}

      {data && <JobTable jobs={data.jobs} />}

      {/* Pagination — always visible when data is loaded */}
      {data && (
        <div className="flex items-center justify-between gap-4 flex-wrap">
          {/* Range info */}
          <p className="text-sm text-muted-foreground tabular-nums">
            {data.total === 0
              ? 'No jobs'
              : `${firstItem.toLocaleString()}–${lastItem.toLocaleString()} of ${data.total.toLocaleString()}`}
          </p>

          {/* Page buttons */}
          {totalPages > 1 && (
            <div className="flex items-center gap-1">
              {/* Previous */}
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                onClick={() => goTo(page - 1)}
                disabled={page === 0}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>

              {/* Page number slots */}
              {buildPageSlots(page, totalPages).map((slot, i) =>
                slot === '…' ? (
                  <span key={`ellipsis-${i}`} className="px-1.5 text-muted-foreground text-sm select-none">
                    …
                  </span>
                ) : (
                  <Button
                    key={slot}
                    variant={slot === page ? 'default' : 'outline'}
                    size="icon"
                    className="h-8 w-8 text-xs"
                    onClick={() => goTo(slot)}
                  >
                    {slot + 1}
                  </Button>
                )
              )}

              {/* Next */}
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                onClick={() => goTo(page + 1)}
                disabled={page >= totalPages - 1}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
