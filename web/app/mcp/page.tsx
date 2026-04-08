'use client'

import { McpTab } from '@/app/mcp/components/mcp-tab'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'

export default function McpPage() {
  usePageGuard('providers')
  const { t } = useTranslation()

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('mcp.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('mcp.description')}</p>
      </div>
      <McpTab />
    </div>
  )
}
