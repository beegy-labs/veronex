'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import { Hash, Coins, CheckCircle, XCircle } from 'lucide-react'
import StatsCard from '@/components/stats-card'

const HOUR_OPTIONS = [6, 12, 24, 48, 72]

function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

function fmtHour(iso: string) {
  const d = new Date(iso)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}h`
}

export default function UsagePage() {
  const [hours, setHours] = useState(24)

  const { data: agg, isLoading: aggLoading, error: aggError } = useQuery({
    queryKey: ['usage-aggregate', hours],
    queryFn: () => api.usageAggregate(hours),
    refetchInterval: 60_000,
  })

  const { data: keys } = useQuery({
    queryKey: ['keys'],
    queryFn: () => api.keys(),
    staleTime: 120_000,
  })

  const [selectedKey, setSelectedKey] = useState<string | null>(null)

  const activeKeyId = selectedKey ?? keys?.[0]?.id ?? null

  const { data: hourly, isLoading: hourlyLoading } = useQuery({
    queryKey: ['key-usage', activeKeyId, hours],
    queryFn: () => api.keyUsage(activeKeyId!, hours),
    enabled: !!activeKeyId,
    refetchInterval: 60_000,
  })

  const chartData = hourly?.map((h) => ({
    hour: fmtHour(h.hour),
    tokens: h.total_tokens,
    requests: h.request_count,
    errors: h.error_count,
  })) ?? []

  const isNoData = !aggLoading && !aggError && agg && agg.request_count === 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">Usage</h1>
          <p className="text-slate-400 mt-1 text-sm">Token consumption and request volume</p>
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
      {aggError && (
        <div className="rounded-xl border border-amber-700 bg-amber-950 p-5 text-amber-300">
          <p className="font-semibold">Analytics unavailable</p>
          <p className="text-sm mt-1 text-amber-400">
            ClickHouse is not enabled. Set <code className="font-mono">CLICKHOUSE_ENABLED=true</code> to track usage.
          </p>
        </div>
      )}

      {/* Aggregate stats */}
      {agg && !aggError && (
        <>
          <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
            <StatsCard
              title="Total Requests"
              value={fmt(agg.request_count)}
              subtitle={`last ${hours}h`}
              icon={<Hash className="h-5 w-5" />}
            />
            <StatsCard
              title="Total Tokens"
              value={fmt(agg.total_tokens)}
              subtitle={`${fmt(agg.prompt_tokens)} prompt · ${fmt(agg.completion_tokens)} completion`}
              icon={<Coins className="h-5 w-5" />}
            />
            <StatsCard
              title="Success"
              value={agg.request_count > 0 ? `${Math.round((agg.success_count / agg.request_count) * 100)}%` : '—'}
              subtitle={`${fmt(agg.success_count)} completed`}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title="Errors"
              value={fmt(agg.error_count)}
              subtitle={`${fmt(agg.cancelled_count)} cancelled`}
              icon={<XCircle className="h-5 w-5" />}
            />
          </div>

          {isNoData && (
            <div className="rounded-xl border border-slate-800 bg-slate-900 p-10 text-center text-slate-500">
              <p className="font-medium">No data yet</p>
              <p className="text-sm mt-1">Submit inference requests to see usage analytics.</p>
            </div>
          )}
        </>
      )}

      {/* Per-key hourly breakdown */}
      {!aggError && keys && keys.length > 0 && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
          <div className="flex items-center justify-between mb-5">
            <h2 className="text-base font-semibold text-slate-200">Hourly Breakdown</h2>
            <select
              value={activeKeyId ?? ''}
              onChange={(e) => setSelectedKey(e.target.value)}
              className="bg-slate-800 border border-slate-700 text-slate-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-indigo-500"
            >
              {keys.map((k) => (
                <option key={k.id} value={k.id}>
                  {k.name} ({k.key_prefix}…)
                </option>
              ))}
            </select>
          </div>

          {hourlyLoading && (
            <div className="flex items-center justify-center h-48 text-slate-400 text-sm">
              Loading…
            </div>
          )}

          {!hourlyLoading && chartData.length === 0 && (
            <div className="flex items-center justify-center h-48 text-slate-500 text-sm">
              No data for this key in the selected time range.
            </div>
          )}

          {!hourlyLoading && chartData.length > 0 && (
            <div className="space-y-8">
              {/* Token area chart */}
              <div>
                <p className="text-xs font-medium text-slate-400 uppercase tracking-wider mb-3">Tokens / Hour</p>
                <ResponsiveContainer width="100%" height={200}>
                  <AreaChart data={chartData}>
                    <defs>
                      <linearGradient id="tokenGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="#6366f1" stopOpacity={0.3} />
                        <stop offset="95%" stopColor="#6366f1" stopOpacity={0} />
                      </linearGradient>
                    </defs>
                    <XAxis dataKey="hour" tick={{ fill: '#94a3b8', fontSize: 11 }} axisLine={false} tickLine={false} />
                    <YAxis tick={{ fill: '#64748b', fontSize: 11 }} axisLine={false} tickLine={false} width={45} tickFormatter={fmt} />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1e293b', border: '1px solid #334155', borderRadius: '8px', color: '#e2e8f0' }}
                      cursor={{ stroke: 'rgba(255,255,255,0.08)' }}
                      formatter={(v: number) => [fmt(v), 'Tokens']}
                    />
                    <Area type="monotone" dataKey="tokens" stroke="#6366f1" fill="url(#tokenGrad)" strokeWidth={2} dot={false} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>

              {/* Request / error bar chart */}
              <div>
                <p className="text-xs font-medium text-slate-400 uppercase tracking-wider mb-3">Requests / Hour</p>
                <ResponsiveContainer width="100%" height={160}>
                  <BarChart data={chartData} barGap={2}>
                    <XAxis dataKey="hour" tick={{ fill: '#94a3b8', fontSize: 11 }} axisLine={false} tickLine={false} />
                    <YAxis tick={{ fill: '#64748b', fontSize: 11 }} axisLine={false} tickLine={false} width={35} />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1e293b', border: '1px solid #334155', borderRadius: '8px', color: '#e2e8f0' }}
                      cursor={{ fill: 'rgba(255,255,255,0.04)' }}
                    />
                    <Legend wrapperStyle={{ fontSize: '12px', color: '#94a3b8' }} />
                    <Bar dataKey="requests" name="Requests" fill="#6366f1" radius={[3, 3, 0, 0]} />
                    <Bar dataKey="errors" name="Errors" fill="#ef4444" radius={[3, 3, 0, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}
        </div>
      )}

      {!aggError && (!keys || keys.length === 0) && !aggLoading && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-6 text-center text-slate-500 text-sm">
          No API keys found. Create one to start tracking per-key usage.
        </div>
      )}
    </div>
  )
}
