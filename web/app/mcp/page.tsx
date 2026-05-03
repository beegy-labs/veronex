'use client'

import { McpTab } from '@/app/mcp/components/mcp-tab'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'

export default function McpPage() {
  usePageGuard('mcp_manage')
  const { t } = useTranslation()

  return (
    <div className="vds-space-y-6">
      <div>
        <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('mcp.title')}</h1>
        <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('mcp.description')}</p>
      </div>
      <McpTab />
    </div>
  )
}
