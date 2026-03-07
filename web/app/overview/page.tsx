'use client'

import { useQuery, useQueries } from '@tanstack/react-query'
import {
  dashboardStatsQuery, recentJobsQuery, performanceQuery,
  usageAggregateQuery, usageBreakdownQuery,
  providersQuery, serversQuery, serverMetricsQuery, serverMetricsHistoryQuery,
} from '@/lib/queries'
import { Card, CardContent } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { DashboardTab } from './components/dashboard-tab'

export default function OverviewPage() {
  const { t } = useTranslation()

  const { data: stats, isLoading: statsLoading, error: statsError } = useQuery(dashboardStatsQuery)
  const { data: providers } = useQuery(providersQuery)
  const { data: servers } = useQuery(serversQuery)

  const serverMetricQueries = useQueries({
    queries: (servers ?? []).map(s => serverMetricsQuery(s.id)),
  })

  const serverHistoryQueries = useQueries({
    queries: (servers ?? []).map(s => serverMetricsHistoryQuery(s.id, 1440)),
  })

  const { data: perf }    = useQuery(performanceQuery(24))
  const { data: perf7d }  = useQuery(performanceQuery(168))
  const { data: perf30d } = useQuery(performanceQuery(720))
  const { data: usage }   = useQuery(usageAggregateQuery(24))
  const { data: breakdown } = useQuery(usageBreakdownQuery(24))
  const { data: recentJobsData } = useQuery(recentJobsQuery)

  if (statsError) {
    return (
      <Card className="border-destructive/50 bg-destructive/10">
        <CardContent className="p-6 text-destructive">
          <p className="font-semibold">{t('overview.failedStats')}</p>
          <p className="text-sm mt-1 opacity-80">
            {statsError instanceof Error ? statsError.message : t('common.unknownError')}
          </p>
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('nav.dashboard')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('overview.description')}</p>
      </div>

      <DashboardTab
        stats={stats}
        statsLoading={statsLoading}
        providers={providers}
        servers={servers}
        serverMetricQueries={serverMetricQueries}
        serverHistoryQueries={serverHistoryQueries}
        perf={perf}
        perf7d={perf7d}
        perf30d={perf30d}
        usage={usage}
        breakdown={breakdown}
        recentJobsData={recentJobsData}
      />
    </div>
  )
}
