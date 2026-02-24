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
      <div className="flex items-center justify-center h-64 text-slate-400">
        Loading stats…
      </div>
    )
  }

  if (error || !stats) {
    return (
      <div className="rounded-xl border border-red-800 bg-red-950 p-6 text-red-300">
        <p className="font-semibold">Failed to load stats</p>
        <p className="text-sm mt-1 text-red-400">
          {error instanceof Error ? error.message : 'Unknown error'}
        </p>
      </div>
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
      <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
        <h2 className="text-base font-semibold text-slate-200 mb-6">Jobs by Status</h2>
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
                backgroundColor: '#1e293b',
                border: '1px solid #334155',
                borderRadius: '8px',
                color: '#e2e8f0',
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
      </div>
    </div>
  )
}
