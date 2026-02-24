'use client'

import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import StatsCard from '@/components/stats-card'
import { Activity, Key, Layers, Clock } from 'lucide-react'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from 'recharts'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

const STATUS_COLORS: Record<string, string> = {
  completed: '#10b981',
  failed:    '#ef4444',
  cancelled: '#6b7280',
  pending:   '#f59e0b',
  running:   '#3b82f6',
}

export default function OverviewPage() {
  const { data: stats, isLoading, error } = useQuery({
    queryKey: ['dashboard-stats'],
    queryFn: () => api.stats(),
    refetchInterval: 30_000,
  })

  if (isLoading) {
    return (
      <div className="flex h-64 items-center justify-center text-muted-foreground">
        Loading stats…
      </div>
    )
  }

  if (error || !stats) {
    return (
      <Card className="border-destructive/50 bg-destructive/10">
        <CardContent className="p-6 text-destructive">
          <p className="font-semibold">Failed to load stats</p>
          <p className="text-sm mt-1 opacity-80">
            {error instanceof Error ? error.message : 'Unknown error'}
          </p>
        </CardContent>
      </Card>
    )
  }

  const chartData = Object.entries(stats.jobs_by_status).map(([status, count]) => ({
    status,
    count,
  }))

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-2xl font-bold text-slate-100">Overview</h1>
        <p className="text-slate-400 mt-1 text-sm">Cluster-wide inference metrics</p>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-4">
        <StatsCard
          title="Total Jobs"
          value={stats.total_jobs}
          icon={<Layers className="h-5 w-5" />}
        />
        <StatsCard
          title="Jobs (last 24h)"
          value={stats.jobs_last_24h}
          icon={<Clock className="h-5 w-5" />}
        />
        <StatsCard
          title="Active Keys"
          value={stats.active_keys}
          subtitle={`${stats.total_keys} total`}
          icon={<Key className="h-5 w-5" />}
        />
        <StatsCard
          title="Completed"
          value={stats.jobs_by_status['completed'] ?? 0}
          subtitle={`${stats.jobs_by_status['failed'] ?? 0} failed`}
          icon={<Activity className="h-5 w-5" />}
        />
      </div>

      {/* Jobs by status chart */}
      <Card>
        <CardHeader>
          <CardTitle>Jobs by Status</CardTitle>
        </CardHeader>
        <CardContent>
          <ResponsiveContainer width="100%" height={240}>
            <BarChart data={chartData} barCategoryGap="30%">
              <XAxis
                dataKey="status"
                tick={{ fill: '#94a3b8', fontSize: 12 }}
                axisLine={false}
                tickLine={false}
              />
              <YAxis
                tick={{ fill: '#64748b', fontSize: 12 }}
                axisLine={false}
                tickLine={false}
                width={40}
              />
              <Tooltip
                contentStyle={{
                  backgroundColor: 'hsl(217 33% 11%)',
                  border: '1px solid hsl(215 28% 17%)',
                  borderRadius: '8px',
                  color: 'hsl(213 31% 91%)',
                }}
                cursor={{ fill: 'rgba(255,255,255,0.04)' }}
              />
              <Bar dataKey="count" radius={[4, 4, 0, 0]}>
                {chartData.map((entry) => (
                  <Cell
                    key={entry.status}
                    fill={STATUS_COLORS[entry.status] ?? '#6366f1'}
                  />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </CardContent>
      </Card>
    </div>
  )
}
