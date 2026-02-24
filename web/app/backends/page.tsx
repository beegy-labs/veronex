'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Backend, RegisterBackendRequest } from '@/lib/types'
import { Plus, Trash2, RefreshCw, X, Server, Key, Wifi, WifiOff, AlertCircle } from 'lucide-react'

// ── Status badge ──────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: Backend['status'] }) {
  if (status === 'online') {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded border text-xs font-medium bg-emerald-900 text-emerald-300 border-emerald-700">
        <Wifi className="h-3 w-3" />
        online
      </span>
    )
  }
  if (status === 'degraded') {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded border text-xs font-medium bg-amber-900 text-amber-300 border-amber-700">
        <AlertCircle className="h-3 w-3" />
        degraded
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded border text-xs font-medium bg-slate-700 text-slate-400 border-slate-600">
      <WifiOff className="h-3 w-3" />
      offline
    </span>
  )
}

// ── Register backend modal ────────────────────────────────────────────────────

function RegisterModal({ onClose }: { onClose: () => void }) {
  const [backendType, setBackendType] = useState<'ollama' | 'gemini'>('ollama')
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [vram, setVram] = useState('')

  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => {
      const body: RegisterBackendRequest = {
        name: name.trim(),
        backend_type: backendType,
        ...(backendType === 'ollama' && { url: url.trim(), total_vram_mb: vram ? parseInt(vram, 10) : undefined }),
        ...(backendType === 'gemini' && { api_key: apiKey.trim() }),
      }
      return api.registerBackend(body)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backends'] })
      onClose()
    },
  })

  const isValid = name.trim() &&
    (backendType === 'ollama' ? url.trim() : apiKey.trim())

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl w-full max-w-md mx-4 p-6">
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-lg font-semibold text-slate-100">Register Backend</h2>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-slate-200"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="space-y-4">
          {/* Backend type toggle */}
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-2">Type</label>
            <div className="grid grid-cols-2 gap-2">
              {(['ollama', 'gemini'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setBackendType(t)}
                  className={`flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-sm font-medium border transition-colors ${
                    backendType === t
                      ? 'bg-indigo-600 border-indigo-500 text-white'
                      : 'bg-slate-800 border-slate-700 text-slate-400 hover:bg-slate-700'
                  }`}
                >
                  {t === 'ollama' ? <Server className="h-4 w-4" /> : <Key className="h-4 w-4" />}
                  {t === 'ollama' ? 'Ollama Server' : 'Gemini API'}
                </button>
              ))}
            </div>
          </div>

          {/* Name */}
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">
              Name <span className="text-red-400">*</span>
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={backendType === 'ollama' ? 'e.g. gpu-server-1' : 'e.g. gemini-prod'}
              className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
            />
          </div>

          {/* Ollama: URL + VRAM */}
          {backendType === 'ollama' && (
            <>
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-1">
                  Ollama URL <span className="text-red-400">*</span>
                </label>
                <input
                  type="url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://192.168.1.10:11434"
                  className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-1">
                  GPU VRAM (MiB)
                  <span className="text-slate-500 font-normal ml-1">— optional, 0 = unknown</span>
                </label>
                <input
                  type="number"
                  value={vram}
                  onChange={(e) => setVram(e.target.value)}
                  placeholder="e.g. 8192"
                  className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                />
              </div>
            </>
          )}

          {/* Gemini: API key */}
          {backendType === 'gemini' && (
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-1">
                Gemini API Key <span className="text-red-400">*</span>
              </label>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="AIza…"
                className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              />
            </div>
          )}
        </div>

        {mutation.error && (
          <div className="mt-4 text-sm text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to register backend'}
          </div>
        )}

        <div className="flex gap-3 mt-6">
          <button
            onClick={onClose}
            className="flex-1 px-4 py-2 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 transition-colors text-sm"
          >
            Cancel
          </button>
          <button
            onClick={() => mutation.mutate()}
            disabled={!isValid || mutation.isPending}
            className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
          >
            {mutation.isPending ? 'Registering…' : 'Register'}
          </button>
        </div>
      </div>
    </div>
  )
}

// ── Page ─────────────────────────────────────────────────────────────────────

export default function BackendsPage() {
  const queryClient = useQueryClient()
  const [showRegister, setShowRegister] = useState(false)

  const { data: backends, isLoading, error } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    refetchInterval: 30_000,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  const healthcheckMutation = useMutation({
    mutationFn: (id: string) => api.healthcheckBackend(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  })

  function handleDelete(id: string, name: string) {
    if (confirm(`Remove backend "${name}"?`)) {
      deleteMutation.mutate(id)
    }
  }

  const ollamaCount = backends?.filter((b) => b.backend_type === 'ollama').length ?? 0
  const geminiCount = backends?.filter((b) => b.backend_type === 'gemini').length ?? 0
  const onlineCount = backends?.filter((b) => b.status === 'online').length ?? 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">Backends</h1>
          <p className="text-slate-400 mt-1 text-sm">
            {backends
              ? `${backends.length} registered — ${ollamaCount} Ollama, ${geminiCount} Gemini — ${onlineCount} online`
              : 'Loading…'}
          </p>
        </div>
        <button
          onClick={() => setShowRegister(true)}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
        >
          <Plus className="h-4 w-4" />
          Register Backend
        </button>
      </div>

      {/* Loading */}
      {isLoading && (
        <div className="flex items-center justify-center h-48 text-slate-400">
          Loading backends…
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="rounded-xl border border-red-800 bg-red-950 p-6 text-red-300">
          <p className="font-semibold">Failed to load backends</p>
          <p className="text-sm mt-1 text-red-400">
            {error instanceof Error ? error.message : 'Unknown error'}
          </p>
        </div>
      )}

      {/* Empty */}
      {backends && backends.length === 0 && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-10 text-center text-slate-500">
          <Server className="h-10 w-10 mx-auto mb-3 opacity-30" />
          <p className="font-medium">No backends registered</p>
          <p className="text-sm mt-1">Add an Ollama server or Gemini API key to start routing inference.</p>
        </div>
      )}

      {/* Table */}
      {backends && backends.length > 0 && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-slate-800 bg-slate-900/80">
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Name</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Type</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">URL / Key</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">VRAM (MiB)</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Status</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Registered</th>
                  <th className="px-4 py-3 text-right font-medium text-slate-400">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-800">
                {backends.map((b) => (
                  <tr key={b.id} className="hover:bg-slate-800/50 transition-colors">
                    <td className="px-4 py-3 text-slate-200 font-medium">{b.name}</td>
                    <td className="px-4 py-3">
                      <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded border text-xs font-medium ${
                        b.backend_type === 'ollama'
                          ? 'bg-blue-900 text-blue-300 border-blue-700'
                          : 'bg-purple-900 text-purple-300 border-purple-700'
                      }`}>
                        {b.backend_type === 'ollama' ? <Server className="h-3 w-3" /> : <Key className="h-3 w-3" />}
                        {b.backend_type}
                      </span>
                    </td>
                    <td className="px-4 py-3 font-mono text-slate-400 text-xs max-w-xs truncate">
                      {b.backend_type === 'ollama' ? b.url : '••••••••••••'}
                    </td>
                    <td className="px-4 py-3 text-slate-400 text-xs tabular-nums">
                      {b.backend_type === 'ollama'
                        ? (b.total_vram_mb === 0 ? '—' : b.total_vram_mb.toLocaleString())
                        : '—'}
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={b.status} />
                    </td>
                    <td className="px-4 py-3 text-slate-400 text-xs">
                      {new Date(b.registered_at).toLocaleDateString()}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <button
                          onClick={() => healthcheckMutation.mutate(b.id)}
                          disabled={healthcheckMutation.isPending}
                          className="p-1.5 rounded hover:bg-slate-700 text-slate-500 hover:text-slate-200 transition-colors disabled:opacity-40"
                          title="Run health check"
                        >
                          <RefreshCw className="h-4 w-4" />
                        </button>
                        <button
                          onClick={() => handleDelete(b.id, b.name)}
                          disabled={deleteMutation.isPending}
                          className="p-1.5 rounded hover:bg-red-900 text-slate-500 hover:text-red-300 transition-colors disabled:opacity-40"
                          title="Remove backend"
                        >
                          <Trash2 className="h-4 w-4" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {showRegister && <RegisterModal onClose={() => setShowRegister(false)} />}
    </div>
  )
}
