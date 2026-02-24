'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Backend, RegisterBackendRequest } from '@/lib/types'
import { Plus, Trash2, RefreshCw, Server, Key, Wifi, WifiOff, AlertCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

// ── Status badge ──────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: Backend['status'] }) {
  if (status === 'online') {
    return (
      <Badge variant="outline" className="bg-emerald-500/15 text-emerald-400 border-emerald-500/30">
        <Wifi className="h-3 w-3 mr-1" />
        online
      </Badge>
    )
  }
  if (status === 'degraded') {
    return (
      <Badge variant="outline" className="bg-amber-500/15 text-amber-400 border-amber-500/30">
        <AlertCircle className="h-3 w-3 mr-1" />
        degraded
      </Badge>
    )
  }
  return (
    <Badge variant="outline" className="bg-muted text-muted-foreground">
      <WifiOff className="h-3 w-3 mr-1" />
      offline
    </Badge>
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
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Register Backend</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          {/* Backend type toggle */}
          <div className="space-y-2">
            <Label>Type</Label>
            <div className="grid grid-cols-2 gap-2">
              {(['ollama', 'gemini'] as const).map((t) => (
                <Button
                  key={t}
                  type="button"
                  variant={backendType === t ? 'default' : 'outline'}
                  onClick={() => setBackendType(t)}
                  className="flex items-center justify-center gap-2"
                >
                  {t === 'ollama' ? <Server className="h-4 w-4" /> : <Key className="h-4 w-4" />}
                  {t === 'ollama' ? 'Ollama Server' : 'Gemini API'}
                </Button>
              ))}
            </div>
          </div>

          {/* Name */}
          <div className="space-y-1.5">
            <Label htmlFor="backend-name">
              Name <span className="text-destructive">*</span>
            </Label>
            <Input
              id="backend-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={backendType === 'ollama' ? 'e.g. gpu-server-1' : 'e.g. gemini-prod'}
            />
          </div>

          {/* Ollama: URL + VRAM */}
          {backendType === 'ollama' && (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="backend-url">
                  Ollama URL <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="backend-url"
                  type="url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://192.168.1.10:11434"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="backend-vram">
                  GPU VRAM (MiB){' '}
                  <span className="text-muted-foreground font-normal">— optional, 0 = unknown</span>
                </Label>
                <Input
                  id="backend-vram"
                  type="number"
                  value={vram}
                  onChange={(e) => setVram(e.target.value)}
                  placeholder="e.g. 8192"
                />
              </div>
            </>
          )}

          {/* Gemini: API key */}
          {backendType === 'gemini' && (
            <div className="space-y-1.5">
              <Label htmlFor="backend-apikey">
                Gemini API Key <span className="text-destructive">*</span>
              </Label>
              <Input
                id="backend-apikey"
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="AIza…"
              />
            </div>
          )}
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to register backend'}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!isValid || mutation.isPending}
          >
            {mutation.isPending ? 'Registering…' : 'Register'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
        <Button onClick={() => setShowRegister(true)}>
          <Plus className="h-4 w-4 mr-2" />
          Register Backend
        </Button>
      </div>

      {/* Loading */}
      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          Loading backends…
        </div>
      )}

      {/* Error */}
      {error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-6 text-destructive">
            <p className="font-semibold">Failed to load backends</p>
            <p className="text-sm mt-1 opacity-80">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </CardContent>
        </Card>
      )}

      {/* Empty */}
      {backends && backends.length === 0 && (
        <Card>
          <CardContent className="p-10 text-center text-muted-foreground">
            <Server className="h-10 w-10 mx-auto mb-3 opacity-30" />
            <p className="font-medium">No backends registered</p>
            <p className="text-sm mt-1">Add an Ollama server or Gemini API key to start routing inference.</p>
          </CardContent>
        </Card>
      )}

      {/* Table */}
      {backends && backends.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>URL / Key</TableHead>
                  <TableHead>VRAM (MiB)</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Registered</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {backends.map((b) => (
                  <TableRow key={b.id}>
                    <TableCell className="font-medium">{b.name}</TableCell>
                    <TableCell>
                      <Badge
                        variant="outline"
                        className={
                          b.backend_type === 'ollama'
                            ? 'bg-blue-500/15 text-blue-400 border-blue-500/30'
                            : 'bg-purple-500/15 text-purple-400 border-purple-500/30'
                        }
                      >
                        {b.backend_type === 'ollama' ? <Server className="h-3 w-3 mr-1" /> : <Key className="h-3 w-3 mr-1" />}
                        {b.backend_type}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-muted-foreground text-xs max-w-xs truncate">
                      {b.backend_type === 'ollama' ? b.url : '••••••••••••'}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs tabular-nums">
                      {b.backend_type === 'ollama'
                        ? (b.total_vram_mb === 0 ? '—' : b.total_vram_mb.toLocaleString())
                        : '—'}
                    </TableCell>
                    <TableCell>
                      <StatusBadge status={b.status} />
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {new Date(b.registered_at).toLocaleDateString()}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => healthcheckMutation.mutate(b.id)}
                          disabled={healthcheckMutation.isPending}
                          title="Run health check"
                        >
                          <RefreshCw className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleDelete(b.id, b.name)}
                          disabled={deleteMutation.isPending}
                          title="Remove backend"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {showRegister && <RegisterModal onClose={() => setShowRegister(false)} />}
    </div>
  )
}
