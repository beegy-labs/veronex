'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { CreateKeyResponse } from '@/lib/types'
import { Plus, Trash2, Copy, Check, X } from 'lucide-react'

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)

  async function handleCopy() {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <button
      onClick={handleCopy}
      className="p-1.5 rounded hover:bg-slate-700 transition-colors text-slate-400 hover:text-slate-200"
      title="Copy to clipboard"
    >
      {copied ? <Check className="h-4 w-4 text-emerald-400" /> : <Copy className="h-4 w-4" />}
    </button>
  )
}

function CreateKeyModal({
  onClose,
  onCreated,
}: {
  onClose: () => void
  onCreated: (resp: CreateKeyResponse) => void
}) {
  const [name, setName] = useState('')
  const [tenantId, setTenantId] = useState('default')
  const [rpm, setRpm] = useState('')
  const [tpm, setTpm] = useState('')

  const mutation = useMutation({
    mutationFn: () =>
      api.createKey({
        name: name.trim(),
        tenant_id: tenantId.trim(),
        rate_limit_rpm: rpm ? parseInt(rpm, 10) : undefined,
        rate_limit_tpm: tpm ? parseInt(tpm, 10) : undefined,
      }),
    onSuccess: (data) => {
      onCreated(data)
    },
  })

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl w-full max-w-md mx-4 p-6">
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-lg font-semibold text-slate-100">Create API Key</h2>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-slate-200"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">
              Name <span className="text-red-400">*</span>
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. production-key"
              className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Tenant ID</label>
            <input
              type="text"
              value={tenantId}
              onChange={(e) => setTenantId(e.target.value)}
              placeholder="default"
              className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-1">
                Rate limit (RPM)
              </label>
              <input
                type="number"
                value={rpm}
                onChange={(e) => setRpm(e.target.value)}
                placeholder="0 = unlimited"
                className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-1">
                Rate limit (TPM)
              </label>
              <input
                type="number"
                value={tpm}
                onChange={(e) => setTpm(e.target.value)}
                placeholder="0 = unlimited"
                className="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              />
            </div>
          </div>
        </div>

        {mutation.error && (
          <div className="mt-4 text-sm text-red-400 bg-red-950 border border-red-800 rounded-lg px-3 py-2">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to create key'}
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
            disabled={!name.trim() || mutation.isPending}
            className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
          >
            {mutation.isPending ? 'Creating…' : 'Create Key'}
          </button>
        </div>
      </div>
    </div>
  )
}

function KeyCreatedModal({
  resp,
  onClose,
}: {
  resp: CreateKeyResponse
  onClose: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl w-full max-w-lg mx-4 p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-slate-100">Key Created</h2>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-slate-200"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="rounded-lg bg-amber-950 border border-amber-700 p-4 mb-4">
          <p className="text-amber-300 text-sm font-medium">
            Save this key now — it will never be shown again.
          </p>
        </div>

        <div className="rounded-lg bg-slate-800 border border-slate-700 p-3 flex items-center gap-2">
          <code className="flex-1 text-emerald-300 font-mono text-sm break-all">
            {resp.key}
          </code>
          <CopyButton text={resp.key} />
        </div>

        <button
          onClick={onClose}
          className="mt-5 w-full px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
        >
          Done
        </button>
      </div>
    </div>
  )
}

export default function KeysPage() {
  const queryClient = useQueryClient()
  const [showCreate, setShowCreate] = useState(false)
  const [createdKey, setCreatedKey] = useState<CreateKeyResponse | null>(null)

  const { data: keys, isLoading, error } = useQuery({
    queryKey: ['keys'],
    queryFn: () => api.keys(),
    refetchInterval: 60_000,
  })

  const revokeMutation = useMutation({
    mutationFn: (id: string) => api.revokeKey(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['keys'] })
    },
  })

  function handleCreated(resp: CreateKeyResponse) {
    setShowCreate(false)
    setCreatedKey(resp)
    queryClient.invalidateQueries({ queryKey: ['keys'] })
  }

  function handleRevoke(id: string, name: string) {
    if (confirm(`Revoke key "${name}"? This cannot be undone.`)) {
      revokeMutation.mutate(id)
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-100">API Keys</h1>
          <p className="text-slate-400 mt-1 text-sm">
            {keys ? `${keys.length} key${keys.length !== 1 ? 's' : ''}` : 'Loading…'}
          </p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
        >
          <Plus className="h-4 w-4" />
          Create Key
        </button>
      </div>

      {isLoading && (
        <div className="flex items-center justify-center h-48 text-slate-400">
          Loading keys…
        </div>
      )}

      {error && (
        <div className="rounded-xl border border-red-800 bg-red-950 p-6 text-red-300">
          <p className="font-semibold">Failed to load keys</p>
          <p className="text-sm mt-1 text-red-400">
            {error instanceof Error ? error.message : 'Unknown error'}
          </p>
        </div>
      )}

      {keys && keys.length === 0 && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-10 text-center text-slate-500">
          No API keys yet. Create one to get started.
        </div>
      )}

      {keys && keys.length > 0 && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-slate-800 bg-slate-900/80">
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Name</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Prefix</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Tenant</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Status</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">RPM / TPM</th>
                  <th className="px-4 py-3 text-left font-medium text-slate-400">Created</th>
                  <th className="px-4 py-3 text-right font-medium text-slate-400">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-800">
                {keys.map((key) => (
                  <tr key={key.id} className="hover:bg-slate-800/50 transition-colors">
                    <td className="px-4 py-3 text-slate-200 font-medium">{key.name}</td>
                    <td className="px-4 py-3 font-mono text-slate-300 text-xs">{key.key_prefix}</td>
                    <td className="px-4 py-3 text-slate-400">{key.tenant_id}</td>
                    <td className="px-4 py-3">
                      <span
                        className={
                          key.is_active
                            ? 'inline-flex items-center px-2 py-0.5 rounded border text-xs font-medium bg-emerald-900 text-emerald-300 border-emerald-700'
                            : 'inline-flex items-center px-2 py-0.5 rounded border text-xs font-medium bg-slate-700 text-slate-400 border-slate-600'
                        }
                      >
                        {key.is_active ? 'active' : 'revoked'}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-slate-400 text-xs tabular-nums">
                      {key.rate_limit_rpm === 0 ? '∞' : key.rate_limit_rpm} /{' '}
                      {key.rate_limit_tpm === 0 ? '∞' : key.rate_limit_tpm}
                    </td>
                    <td className="px-4 py-3 text-slate-400 text-xs">
                      {new Date(key.created_at).toLocaleDateString()}
                    </td>
                    <td className="px-4 py-3 text-right">
                      {key.is_active && (
                        <button
                          onClick={() => handleRevoke(key.id, key.name)}
                          disabled={revokeMutation.isPending}
                          className="p-1.5 rounded hover:bg-red-900 text-slate-500 hover:text-red-300 transition-colors disabled:opacity-40"
                          title="Revoke key"
                        >
                          <Trash2 className="h-4 w-4" />
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {showCreate && (
        <CreateKeyModal
          onClose={() => setShowCreate(false)}
          onCreated={handleCreated}
        />
      )}

      {createdKey && (
        <KeyCreatedModal
          resp={createdKey}
          onClose={() => setCreatedKey(null)}
        />
      )}
    </div>
  )
}
