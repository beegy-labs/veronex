'use client'

import type { Backend, GpuServer } from '@/lib/types'
import { useInferenceStream } from '@/hooks/use-inference-stream'
import { ProviderFlowPanel } from './provider-flow-panel'
import { LiveFeed } from './live-feed'

interface Props {
  backends: Backend[]
  servers: GpuServer[]
}

export function NetworkFlowTab({ backends, servers }: Props) {
  const events = useInferenceStream(backends, servers)

  return (
    <div className="space-y-4">
      <ProviderFlowPanel backends={backends} servers={servers} events={events} />
      <LiveFeed events={events} />
    </div>
  )
}
