'use client'

import { useQuery } from '@tanstack/react-query'
import type { Provider } from '@/lib/types'
import { queueDepthQuery } from '@/lib/queries'
import { useInferenceStream } from '@/hooks/use-inference-stream'
import { ProviderFlowPanel } from './provider-flow-panel'
import { LiveFeed } from './live-feed'

interface Props {
  backends: Provider[]
}

export function NetworkFlowTab({ backends }: Props) {
  const events = useInferenceStream()
  const { data: depth } = useQuery(queueDepthQuery)

  return (
    <div className="space-y-4">
      <ProviderFlowPanel backends={backends} events={events} queueDepth={depth?.total ?? 0} />
      <LiveFeed events={events} />
    </div>
  )
}
