'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { ApiKey, CreateKeyResponse } from '@/lib/types'
import { Plus, Trash2, Copy, Check } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
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
import { useTranslation } from '@/i18n'

function CopyButton({ text }: { text: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  async function handleCopy() {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <Button variant="ghost" size="icon" onClick={handleCopy} title={t('common.copy')}>
      {copied ? <Check className="h-4 w-4 text-status-success-fg" /> : <Copy className="h-4 w-4" />}
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
  const { t } = useTranslation()
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
    onSuccess: (data) => onCreated(data),
  })

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('keys.createTitle')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="key-name">{t('keys.keyName')} <span className="text-destructive">*</span></Label>
            <Input
              id="key-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('keys.keyNamePlaceholder')}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="key-tenant">{t('keys.tenantId')}</Label>
            <Input
              id="key-tenant"
              value={tenantId}
              onChange={(e) => setTenantId(e.target.value)}
              placeholder="default"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="key-rpm">{t('keys.rateLimitRpm')}</Label>
              <Input
                id="key-rpm"
                type="number"
                value={rpm}
                onChange={(e) => setRpm(e.target.value)}
                placeholder={t('keys.rateLimitPlaceholder')}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="key-tpm">{t('keys.rateLimitTpm')}</Label>
              <Input
                id="key-tpm"
                type="number"
                value={tpm}
                onChange={(e) => setTpm(e.target.value)}
                placeholder={t('keys.rateLimitPlaceholder')}
              />
            </div>
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.unknownError')}
          </p>
        )}

        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!name.trim() || mutation.isPending}>
            {mutation.isPending ? t('keys.creating') : t('keys.createKey')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function KeyCreatedModal({ resp, onClose }: { resp: CreateKeyResponse; onClose: () => void }) {
  const { t } = useTranslation()
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('keys.createdTitle')}</DialogTitle>
        </DialogHeader>

        <div className="rounded-lg border border-status-warning/30 bg-status-warning/10 p-4 text-status-warning-fg text-sm">
          {t('keys.createdWarning')}
        </div>

        <div className="rounded-lg bg-muted p-3 flex items-center gap-2">
          <code className="flex-1 font-mono text-sm text-status-success-fg break-all">{resp.key}</code>
          <CopyButton text={resp.key} />
        </div>

        <DialogFooter>
          <Button onClick={onClose}>{t('common.done')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function DeleteConfirmModal({
  keyName,
  onConfirm,
  onClose,
  isPending,
}: {
  keyName: string
  onConfirm: () => void
  onClose: () => void
  isPending: boolean
}) {
  const { t } = useTranslation()
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t('keys.deleteTitle')}</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          {t('keys.deleteConfirm', { name: keyName })}
        </p>
        <DialogFooter className="gap-3">
          <Button variant="outline" onClick={onClose} disabled={isPending}>{t('common.cancel')}</Button>
          <Button variant="destructive" onClick={onConfirm} disabled={isPending}>
            {isPending ? t('keys.deleting') : t('common.delete')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default function KeysPage() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [showCreate, setShowCreate] = useState(false)
  const [createdKey, setCreatedKey] = useState<CreateKeyResponse | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null)

  const { data: keys, isLoading, error } = useQuery({
    queryKey: ['keys'],
    queryFn: () => api.keys(),
    refetchInterval: 60_000,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteKey(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['keys'] })
      setDeleteTarget(null)
    },
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, is_active }: { id: string; is_active: boolean }) =>
      api.toggleKeyActive(id, is_active),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['keys'] }),
  })

  function handleCreated(resp: CreateKeyResponse) {
    setShowCreate(false)
    setCreatedKey(resp)
    queryClient.invalidateQueries({ queryKey: ['keys'] })
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('keys.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">
            {keys ? `${keys.length} key${keys.length !== 1 ? 's' : ''}` : t('common.loading')}
          </p>
        </div>
        <Button onClick={() => setShowCreate(true)}>
          <Plus className="h-4 w-4 mr-2" />
          {t('keys.createKey')}
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          {t('keys.loadingKeys')}
        </div>
      )}

      {error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-6 text-destructive">
            <p className="font-semibold">{t('keys.failedKeys')}</p>
            <p className="text-sm mt-1 opacity-80">
              {error instanceof Error ? error.message : t('common.unknownError')}
            </p>
          </CardContent>
        </Card>
      )}

      {keys && keys.length === 0 && (
        <Card>
          <CardContent className="p-10 text-center text-muted-foreground">
            {t('keys.noKeys')}
          </CardContent>
        </Card>
      )}

      {keys && keys.length > 0 && (
        <Card>
          <CardContent className="p-0 overflow-x-auto">
            <Table className="min-w-[700px]">
              <TableHeader>
                <TableRow>
                  <TableHead>{t('keys.name')}</TableHead>
                  <TableHead>{t('keys.prefix')}</TableHead>
                  <TableHead>{t('keys.tenant')}</TableHead>
                  <TableHead>{t('keys.status')}</TableHead>
                  <TableHead>{t('keys.activeToggle')}</TableHead>
                  <TableHead>{t('keys.rpmTpm')}</TableHead>
                  <TableHead>{t('keys.createdAt')}</TableHead>
                  <TableHead className="text-right">{t('keys.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keys.map((key) => (
                  <TableRow key={key.id} className={!key.is_active ? 'opacity-50' : ''}>
                    <TableCell className="font-medium">{key.name}</TableCell>
                    <TableCell className="font-mono text-xs">{key.key_prefix}</TableCell>
                    <TableCell className="text-muted-foreground">{key.tenant_id}</TableCell>
                    <TableCell>
                      <Badge
                        variant="outline"
                        className={
                          key.is_active
                            ? 'bg-status-success/15 text-status-success-fg border-status-success/30'
                            : 'bg-muted text-muted-foreground'
                        }
                      >
                        {key.is_active ? t('common.active') : t('common.inactive')}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <Switch
                        checked={key.is_active}
                        onCheckedChange={(checked) =>
                          toggleMutation.mutate({ id: key.id, is_active: checked })
                        }
                        disabled={toggleMutation.isPending}
                      />
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs tabular-nums">
                      {key.rate_limit_rpm === 0 ? '∞' : key.rate_limit_rpm} /{' '}
                      {key.rate_limit_tpm === 0 ? '∞' : key.rate_limit_tpm}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {new Date(key.created_at).toLocaleDateString()}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => setDeleteTarget(key)}
                        disabled={deleteMutation.isPending}
                        title={t('keys.deleteKey')}
                        className="text-muted-foreground hover:text-destructive"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {showCreate && (
        <CreateKeyModal onClose={() => setShowCreate(false)} onCreated={handleCreated} />
      )}

      {createdKey && (
        <KeyCreatedModal resp={createdKey} onClose={() => setCreatedKey(null)} />
      )}

      {deleteTarget && (
        <DeleteConfirmModal
          keyName={deleteTarget.name}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isPending={deleteMutation.isPending}
        />
      )}
    </div>
  )
}
