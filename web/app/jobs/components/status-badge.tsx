'use client'

import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'
import { STATUS_STYLES } from '@/lib/constants'

export function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  const key = `jobs.statuses.${status}` as Parameters<typeof t>[0]
  return (
    <Badge
      variant="outline"
      className={`whitespace-nowrap ${STATUS_STYLES[status] ?? 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30'}`}
    >
      {t(key)}
    </Badge>
  )
}
