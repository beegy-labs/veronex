'use client'

import { useQuery } from '@tanstack/react-query'
import { backendsQuery, serversQuery } from '@/lib/queries'
import { useTranslation } from '@/i18n'
import { NetworkFlowTab } from '@/app/overview/components/network-flow-tab'

export default function FlowPage() {
  const { t } = useTranslation()

  const { data: backends } = useQuery(backendsQuery)
  const { data: servers } = useQuery(serversQuery)

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('nav.flow')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('overview.networkFlowDesc')}</p>
      </div>

      <NetworkFlowTab
        backends={backends ?? []}
        servers={servers ?? []}
      />
    </div>
  )
}
