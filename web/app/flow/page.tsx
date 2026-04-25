'use client'

import { useQuery } from '@tanstack/react-query'
import { providersQuery } from '@/lib/queries'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { NetworkFlowTab } from '@/components/network-flow-tab'

export default function FlowPage() {
  usePageGuard('dashboard_view')
  const { t } = useTranslation()

  const { data: providersData } = useQuery(providersQuery())
  const providers = providersData?.providers

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('nav.flow')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('overview.networkFlowDesc')}</p>
      </div>

      <NetworkFlowTab providers={providers ?? []} />
    </div>
  )
}
