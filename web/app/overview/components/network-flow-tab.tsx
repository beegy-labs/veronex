'use client'

import { memo, useMemo } from 'react'
import type { Provider } from '@/lib/types'
import { useInferenceStream } from '@/hooks/use-inference-stream'
import { ProviderFlowPanel } from './provider-flow-panel'
import { LiveFeed } from './live-feed'

interface Props {
  providers: Provider[]
}

export const NetworkFlowTab = memo(function NetworkFlowTab({ providers }: Props) {
  const { events, stats } = useInferenceStream()

  // Compute counts from events (fallback when server stats unavailable or stale)
  const derived = useMemo(() => {
    const now = Date.now()
    const jobs = new Map<string, string>()
    let reqLastSec = 0

    for (const e of events) {
      jobs.set(e.jobId, e.status)
      // Count enqueue events in last 10 seconds for smoothed rate
      if (e.phase === 'enqueue' && now - e.ts < 10_000) {
        reqLastSec++
      }
    }

    let pending = 0
    let running = 0
    for (const status of jobs.values()) {
      if (status === 'pending') pending++
      else if (status === 'running') running++
    }

    // req/s = enqueue events in last 10s / 10
    const reqPerSec = Math.round(reqLastSec / 10 * 10) / 10

    return { pending, running, reqPerSec }
  }, [events])

  // Prefer server stats when available, fallback to client-derived
  const pendingJobs = stats?.queued ?? derived.pending
  const runningJobs = stats?.running ?? derived.running
  const recentRequests = stats?.incoming ?? derived.reqPerSec

  return (
    <div className="space-y-4">
      <ProviderFlowPanel
        providers={providers}
        events={events}
        pendingJobs={pendingJobs}
        runningJobs={runningJobs}
        recentRequests={recentRequests}
      />
      <LiveFeed events={events} />
    </div>
  )
})
