'use client'

import { McpTab } from '@/app/providers/components/mcp-tab'
import { useTranslation } from '@/i18n'

export default function McpPage() {
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
