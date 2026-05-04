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
    <div className="vds-space-y-6">
      <div>
        <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('nav.flow')}</h1>
        <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('overview.networkFlowDesc')}</p>
      </div>

      <NetworkFlowTab providers={providers ?? []} />
    </div>
  )
}
