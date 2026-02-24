'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import {
  LineChart, Line, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer,
  ReferenceLine, Legend,
} from 'recharts'
import { Timer, TrendingUp, CheckCircle, Zap } from 'lucide-react'
import StatsCard from '@/components/stats-card'

const HOUR_OPTIONS = [6, 12, 24, 48, 72]

function ms(n: number) {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}s`
  return `${Math.round(n)}ms`
}

function pct(n: number) {
  return `${Math.round(n * 100)}%`
}

function fmtHour(iso: string) {
  const d = new Date(iso)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}h`
}

export default function PerformancePage() {
  const [hours, setHours] = useState(24)

  const { data, isLoading, error } = useQuery({
    queryKey: ['performance', hours],
    queryFn: () => api.performance(hours),
    refetchInterval: 60_000,
  })

  const latencyCardData = data
    ? [
        { label: 'Avg', value: ms(data.avg_latency_ms) },
        { label: 'P50', value: ms(data.p50_latency_ms) },
        { label: 'P95', value: ms(data.p95_latency_ms) },
        { label: 'P99', value: ms(data.p99_latency_ms) },
      ]
    : []

  const chartData = data?.hourly.map((h) => ({
    hour: fmtHour(h.hour),
    latency: Math.round(h.avg_latency_ms),
    requests: h.request_count,
    success: h.success_count,
    tokens: h.total_tokens,
  })) ?? []

  const hasData = data && data.total_requests > 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">Performance</h1>
          <p className="text-slate-400 mt-1 text-sm">Latency percentiles and throughput</p>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-slate-400">Last</span>
          {HOUR_OPTIONS.map((h) => (
            <button
              key={h}
              onClick={() => setHours(h)}
              className={`px-3 py-1 rounded text-sm font-medium transition-colors ${
                hours === h
                  ? 'bg-indigo-600 text-white'
                  : 'bg-slate-800 text-slate-400 hover:bg-slate-700'
              }`}
            >
              {h}h
            </button>
          ))}
        </div>
      </div>

      {/* ClickHouse unavailable */}
      {error && (
        <div className="rounded-xl border border-amber-700 bg-amber-950 p-5 text-amber-300">
          <p className="font-semibold">Performance analytics unavailable</p>
          <p className="text-sm mt-1 text-amber-400">
            ClickHouse is not enabled. Set <code className="font-mono">CLICKHOUSE_ENABLED=true</code> to track latency and throughput.
          </p>
        </div>
      )}

      {isLoading && (
        <div className="flex items-center justify-center h-48 text-slate-400">Loading…</div>
      )}

      {!error && !isLoading && !hasData && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-10 text-center text-slate-500">
          <p className="font-medium">No data yet</p>
          <p className="text-sm mt-1">Submit inference requests to see performance metrics.</p>
        </div>
      )}

      {!error && data && hasData && (
        <>
          {/* Summary cards */}
          <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
            <StatsCard
              title="Avg Latency"
              value={ms(data.avg_latency_ms)}
              subtitle={`P50 ${ms(data.p50_latency_ms)}`}
              icon={<Timer className="h-5 w-5" />}
            />
            <StatsCard
              title="P95 / P99"
              value={ms(data.p95_latency_ms)}
              subtitle={`P99 ${ms(data.p99_latency_ms)}`}
              icon={<TrendingUp className="h-5 w-5" />}
            />
            <StatsCard
              title="Success Rate"
              value={pct(data.success_rate)}
              subtitle={`${data.total_requests.toLocaleString()} total requests`}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title="Total Tokens"
              value={data.total_tokens >= 1000 ? `${(data.total_tokens / 1000).toFixed(1)}K` : String(data.total_tokens)}
              subtitle={`last ${hours}h`}
              icon={<Zap className="h-5 w-5" />}
            />
          </div>

          {/* Latency percentile bars */}
          <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
            <h2 className="text-base font-semibold text-slate-200 mb-2">Latency Percentiles</h2>
            <p className="text-xs text-slate-500 mb-5">Aggregated over the selected time range</p>
            <div className="grid grid-cols-4 gap-3">
              {latencyCardData.map(({ label, value }) => (
                <div key={label} className="rounded-lg bg-slate-800 border border-slate-700 p-4 text-center">
                  <p className="text-xs text-slate-400 font-medium mb-1">{label}</p>
                  <p className="text-xl font-bold text-slate-100 font-mono">{value}</p>
                </div>
              ))}
            </div>
          </div>

          {chartData.length > 0 && (
            <>
              {/* Avg latency over time */}
              <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
                <h2 className="text-base font-semibold text-slate-200 mb-5">Avg Latency / Hour</h2>
                <ResponsiveContainer width="100%" height={200}>
                  <LineChart data={chartData}>
                    <XAxis dataKey="hour" tick={{ fill: '#94a3b8', fontSize: 11 }} axisLine={false} tickLine={false} />
                    <YAxis
                      tick={{ fill: '#64748b', fontSize: 11 }}
                      axisLine={false}
                      tickLine={false}
                      width={55}
                      tickFormatter={(v) => ms(v)}
                    />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1e293b', border: '1px solid #334155', borderRadius: '8px', color: '#e2e8f0' }}
                      cursor={{ stroke: 'rgba(255,255,255,0.08)' }}
                      formatter={(v: number) => [ms(v), 'Avg latency']}
                    />
                    <ReferenceLine
                      y={data.p95_latency_ms}
                      stroke="#f59e0b"
                      strokeDasharray="4 4"
                      label={{ value: 'P95', position: 'right', fill: '#f59e0b', fontSize: 11 }}
                    />
                    <Line type="monotone" dataKey="latency" stroke="#6366f1" strokeWidth={2} dot={false} />
                  </LineChart>
                </ResponsiveContainer>
              </div>

              {/* Throughput (requests / success) */}
              <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
                <h2 className="text-base font-semibold text-slate-200 mb-5">Throughput / Hour</h2>
                <ResponsiveContainer width="100%" height={180}>
                  <BarChart data={chartData} barGap={2}>
                    <XAxis dataKey="hour" tick={{ fill: '#94a3b8', fontSize: 11 }} axisLine={false} tickLine={false} />
                    <YAxis tick={{ fill: '#64748b', fontSize: 11 }} axisLine={false} tickLine={false} width={35} />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1e293b', border: '1px solid #334155', borderRadius: '8px', color: '#e2e8f0' }}
                      cursor={{ fill: 'rgba(255,255,255,0.04)' }}
                    />
                    <Legend wrapperStyle={{ fontSize: '12px', color: '#94a3b8' }} />
                    <Bar dataKey="requests" name="Total" fill="#6366f1" radius={[3, 3, 0, 0]} />
                    <Bar dataKey="success" name="Success" fill="#10b981" radius={[3, 3, 0, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </>
          )}
        </>
      )}
    </div>
  )
}
