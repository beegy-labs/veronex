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

  return (
    <div className="space-y-4">
      <ProviderFlowPanel
        providers={providers}
        events={events}
        pendingJobs={stats?.queued ?? 0}
        runningJobs={stats?.running ?? 0}
        recentRequests={stats?.incoming ?? 0}
      />
      <LiveFeed events={events} />
    </div>
  )
})
