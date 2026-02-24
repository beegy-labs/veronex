'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { CreateKeyResponse } from '@/lib/types'
import { Plus, Trash2, Copy, Check } from 'lucide-react'
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

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)

  async function handleCopy() {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={handleCopy}
      title="Copy to clipboard"
    >
      {copied ? <Check className="h-4 w-4 text-emerald-400" /> : <Copy className="h-4 w-4" />}
    </Button>
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
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Create API Key</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="key-name">
              Name <span className="text-destructive">*</span>
            </Label>
            <Input
              id="key-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. production-key"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="key-tenant">Tenant ID</Label>
            <Input
              id="key-tenant"
              type="text"
              value={tenantId}
              onChange={(e) => setTenantId(e.target.value)}
              placeholder="default"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="key-rpm">Rate limit (RPM)</Label>
              <Input
                id="key-rpm"
                type="number"
                value={rpm}
                onChange={(e) => setRpm(e.target.value)}
                placeholder="0 = unlimited"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="key-tpm">Rate limit (TPM)</Label>
              <Input
                id="key-tpm"
                type="number"
                value={tpm}
                onChange={(e) => setTpm(e.target.value)}
                placeholder="0 = unlimited"
              />
            </div>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : 'Failed to create key'}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!name.trim() || mutation.isPending}
          >
            {mutation.isPending ? 'Creating…' : 'Create Key'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Key Created</DialogTitle>
        </DialogHeader>

        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-4 text-amber-400 text-sm">
          Save this key now — it will never be shown again.
        </div>

        <div className="rounded-lg bg-muted p-3 flex items-center gap-2">
          <code className="flex-1 font-mono text-sm text-emerald-400 break-all">
            {resp.key}
          </code>
          <CopyButton text={resp.key} />
        </div>

        <DialogFooter>
          <Button onClick={onClose}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
        <Button onClick={() => setShowCreate(true)}>
          <Plus className="h-4 w-4 mr-2" />
          Create Key
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          Loading keys…
        </div>
      )}

      {error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-6 text-destructive">
            <p className="font-semibold">Failed to load keys</p>
            <p className="text-sm mt-1 opacity-80">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </CardContent>
        </Card>
      )}

      {keys && keys.length === 0 && (
        <Card>
          <CardContent className="p-10 text-center text-muted-foreground">
            No API keys yet. Create one to get started.
          </CardContent>
        </Card>
      )}

      {keys && keys.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Prefix</TableHead>
                  <TableHead>Tenant</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>RPM / TPM</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keys.map((key) => (
                  <TableRow key={key.id}>
                    <TableCell className="font-medium">{key.name}</TableCell>
                    <TableCell className="font-mono text-xs">{key.key_prefix}</TableCell>
                    <TableCell className="text-muted-foreground">{key.tenant_id}</TableCell>
                    <TableCell>
                      <Badge
                        variant="outline"
                        className={
                          key.is_active
                            ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30'
                            : 'bg-muted text-muted-foreground'
                        }
                      >
                        {key.is_active ? 'active' : 'revoked'}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs tabular-nums">
                      {key.rate_limit_rpm === 0 ? '∞' : key.rate_limit_rpm} /{' '}
                      {key.rate_limit_tpm === 0 ? '∞' : key.rate_limit_tpm}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {new Date(key.created_at).toLocaleDateString()}
                    </TableCell>
                    <TableCell className="text-right">
                      {key.is_active && (
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleRevoke(key.id, key.name)}
                          disabled={revokeMutation.isPending}
                          title="Revoke key"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
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
