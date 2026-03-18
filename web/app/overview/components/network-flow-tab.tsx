'use client'

import { memo } from 'react'
import type { Provider } from '@/lib/types'
import { useInferenceStream } from '@/hooks/use-inference-stream'
import { ProviderFlowPanel } from './provider-flow-panel'
import { LiveFeed } from './live-feed'

interface Props {
  providers: Provider[]
}

export const NetworkFlowTab = memo(function NetworkFlowTab({ providers }: Props) {
  const { events, stats } = useInferenceStream()

  // Server stats are authoritative — pushed every second via SSE.
  // No client-derived fallback: avoids stale replay events causing 23→0 flicker.
  // Before first flow_stats arrives (~1s), show 0.
  const pendingJobs = stats?.queued ?? 0
  const runningJobs = stats?.running ?? 0
  const recentRequests = stats != null ? stats.incoming / 10 : 0
  const reqPerMin = stats?.incoming_60s ?? 0

  return (
    <div className="space-y-4">
      <ProviderFlowPanel
        providers={providers}
        events={events}
        pendingJobs={pendingJobs}
        runningJobs={runningJobs}
        recentRequests={recentRequests}
        reqPerMin={reqPerMin}
      />
      <LiveFeed />
    </div>
  )
})
