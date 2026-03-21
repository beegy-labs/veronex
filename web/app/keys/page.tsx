'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { keysQuery, resourceAuditQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { ApiKey, CreateKeyResponse } from '@/lib/types'
import { Plus, Trash2, BarChart2, RefreshCw, History } from 'lucide-react'
import { CopyButton } from '@/components/copy-button'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable, DataTableEmpty } from '@/components/data-table'
import { KeyUsageModal } from '@/components/key-usage-modal'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDateOnly } from '@/lib/date'

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
  const [tier, setTier] = useState<'free' | 'paid'>('paid')

  const mutation = useMutation({
    mutationFn: () =>
      api.createKey({
        name: name.trim(),
        tenant_id: tenantId.trim(),
        rate_limit_rpm: rpm ? parseInt(rpm, 10) : undefined,
        rate_limit_tpm: tpm ? parseInt(tpm, 10) : undefined,
        tier,
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
              placeholder={t('keys.tenantIdPlaceholder')}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="key-tier">{t('keys.tier')}</Label>
            <Select value={tier} onValueChange={(v) => setTier(v as 'free' | 'paid')}>
              <SelectTrigger id="key-tier">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="paid">{t('keys.tierPaid')}</SelectItem>
                <SelectItem value="free">{t('keys.tierFree')}</SelectItem>
              </SelectContent>
            </Select>
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

        <DialogFooter className="gap-3 flex-wrap">
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
  const [ack, setAck] = useState(false)

  return (
    <Dialog open onOpenChange={() => { /* block dismiss until ack */ }}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{t('keys.createdTitle')}</DialogTitle>
        </DialogHeader>

        <div className="rounded-lg border border-status-warning/30 bg-status-warning/10 p-4 text-status-warning-fg text-sm">
          {t('keys.createdWarning')}
        </div>

        <div className="rounded-lg bg-muted p-3 flex items-center gap-2">
          <code className="flex-1 font-mono text-sm text-status-success-fg break-all select-all">{resp.key}</code>
          <CopyButton text={resp.key} />
        </div>

        <div className="flex items-center gap-2">
          <Checkbox id="key-ack" checked={ack} onCheckedChange={(v) => setAck(v === true)} />
          <Label htmlFor="key-ack" className="text-sm cursor-pointer">{t('keys.keySavedAck')}</Label>
        </div>

        <DialogFooter>
          <Button onClick={onClose} disabled={!ack}>{t('common.done')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function KeyHistoryModal({ apiKey, onClose }: { apiKey: ApiKey; onClose: () => void }) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const { data: events, isLoading } = useQuery(resourceAuditQuery('api_key', apiKey.id))

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-lg max-h-[70vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t('keys.historyTitle', { name: apiKey.name })}</DialogTitle>
        </DialogHeader>
        <div className="flex-1 overflow-y-auto space-y-2 min-h-0">
          {isLoading && <p className="text-sm text-muted-foreground">{t('common.loading')}</p>}
          {events && events.length === 0 && (
            <p className="text-sm text-muted-foreground">{t('common.empty')}</p>
          )}
          {events?.map((ev) => (
            <div key={`${ev.event_time}-${ev.account_id}-${ev.action}-${ev.resource_id}`} className="rounded-lg border px-3 py-2 text-sm space-y-0.5">
              <div className="flex items-center justify-between gap-2">
                <Badge variant="outline" className="text-[10px]">{ev.action}</Badge>
                <span className="text-xs text-muted-foreground">{fmtDateOnly(ev.event_time, tz)}</span>
              </div>
              <p className="text-xs text-muted-foreground">{ev.details}</p>
              <p className="text-[10px] text-muted-foreground/60">{ev.account_name}</p>
            </div>
          ))}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default function KeysPage() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const queryClient = useQueryClient()
  const [showCreate, setShowCreate] = useState(false)
  const [createdKey, setCreatedKey] = useState<CreateKeyResponse | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null)
  const [regenerateTarget, setRegenerateTarget] = useState<ApiKey | null>(null)
  const [usageKey, setUsageKey] = useState<ApiKey | null>(null)
  const [historyKey, setHistoryKey] = useState<ApiKey | null>(null)

  const { data: keys, isLoading, error } = useQuery(keysQuery)

  const hasCreatedBy = keys?.some((k) => k.created_by)

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteKey(id),
    { invalidateKey: ['keys'], onSuccess: () => setDeleteTarget(null) },
  )

  const toggleMutation = useApiMutation(
    (vars: { id: string; is_active: boolean }) => api.toggleKeyActive(vars.id, vars.is_active),
    { invalidateKey: ['keys'] },
  )

  const tierMutation = useApiMutation(
    (vars: { id: string; tier: 'free' | 'paid' }) => api.updateKeyTier(vars.id, vars.tier),
    { invalidateKey: ['keys'] },
  )

  const regenerateMutation = useApiMutation(
    (id: string) => api.regenerateKey(id),
    {
      invalidateKey: ['keys'],
      onSuccess: (data) => { setRegenerateTarget(null); setCreatedKey(data) },
    },
  )

  function handleCreated(resp: CreateKeyResponse) {
    setShowCreate(false)
    setCreatedKey(resp)
    queryClient.invalidateQueries({ queryKey: ['keys'] })
  }

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('keys.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">
            {keys
              ? t('keys.keysCount', { count: keys.length })
              : t('common.loading')}
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

      {keys && (
        keys.length === 0
          ? <DataTableEmpty>{t('keys.noKeys')}</DataTableEmpty>
          : (
            <DataTable minWidth="720px">
              <TableHeader>
                <TableRow>
                  <TableHead className="whitespace-nowrap">{t('keys.name')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.prefix')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.tenant')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.tier')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.status')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.activeToggle')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('keys.rpmTpm')}</TableHead>
                  {hasCreatedBy && <TableHead className="whitespace-nowrap">{t('keys.createdBy')}</TableHead>}
                  <TableHead className="whitespace-nowrap">{t('keys.createdAt')}</TableHead>
                  <TableHead className="text-right whitespace-nowrap">{t('keys.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keys.map((key) => (
                  <TableRow key={key.id} className={!key.is_active ? 'opacity-50' : ''}>
                    <TableCell className="font-medium">{key.name}</TableCell>
                    <TableCell className="font-mono text-xs">{key.key_prefix}</TableCell>
                    <TableCell className="text-muted-foreground">{key.tenant_id}</TableCell>
                    <TableCell>
                      <Select
                        value={key.tier}
                        onValueChange={(v) =>
                          tierMutation.mutate({ id: key.id, tier: v as 'free' | 'paid' })
                        }
                        disabled={tierMutation.isPending}
                      >
                        <SelectTrigger className="h-7 w-24 text-xs">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="paid">{t('keys.tierPaid')}</SelectItem>
                          <SelectItem value="free">{t('keys.tierFree')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant="outline"
                        className={`whitespace-nowrap ${
                          key.is_active
                            ? 'bg-status-success/15 text-status-success-fg border-status-success/30'
                            : 'bg-muted text-muted-foreground'
                        }`}
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
                        aria-label={key.is_active ? t('common.deactivate') : t('common.activate')}
                      />
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs tabular-nums">
                      {key.rate_limit_rpm === 0 ? '∞' : key.rate_limit_rpm} /{' '}
                      {key.rate_limit_tpm === 0 ? '∞' : key.rate_limit_tpm}
                    </TableCell>
                    {hasCreatedBy && (
                      <TableCell className="text-muted-foreground text-xs">
                        {key.created_by ?? '—'}
                      </TableCell>
                    )}
                    <TableCell className="text-muted-foreground text-xs">
                      {fmtDateOnly(key.created_at, tz)}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.viewUsage')}
                          onClick={() => setUsageKey(key)}
                          title={t('keys.viewUsage')}
                          className="text-muted-foreground hover:text-primary"
                        >
                          <BarChart2 className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.viewHistory')}
                          onClick={() => setHistoryKey(key)}
                          title={t('keys.viewHistory')}
                          className="text-muted-foreground hover:text-primary"
                        >
                          <History className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.regenerateKey')}
                          onClick={() => setRegenerateTarget(key)}
                          title={t('keys.regenerateKey')}
                          className="text-muted-foreground hover:text-status-warning-fg"
                        >
                          <RefreshCw className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.deleteKey')}
                          onClick={() => setDeleteTarget(key)}
                          disabled={deleteMutation.isPending}
                          title={t('keys.deleteKey')}
                          className="text-muted-foreground hover:text-destructive"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </DataTable>
          )
      )}

      {showCreate && (
        <CreateKeyModal
          onClose={() => setShowCreate(false)}
          onCreated={handleCreated}
        />
      )}

      {createdKey && (
        <KeyCreatedModal resp={createdKey} onClose={() => setCreatedKey(null)} />
      )}

      {deleteTarget && (
        <ConfirmDialog
          open
          title={t('keys.deleteTitle')}
          description={t('keys.deleteConfirm', { name: deleteTarget.name })}
          confirmLabel={deleteMutation.isPending ? t('keys.deleting') : t('common.delete')}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isLoading={deleteMutation.isPending}
        />
      )}

      {regenerateTarget && (
        <ConfirmDialog
          open
          title={t('keys.regenerateTitle')}
          description={t('keys.regenerateConfirm', { name: regenerateTarget.name })}
          confirmLabel={regenerateMutation.isPending ? t('keys.regenerating') : t('keys.regenerateKey')}
          onConfirm={() => regenerateMutation.mutate(regenerateTarget.id)}
          onClose={() => setRegenerateTarget(null)}
          isLoading={regenerateMutation.isPending}
        />
      )}

      {usageKey && (
        <KeyUsageModal apiKey={usageKey} onClose={() => setUsageKey(null)} />
      )}

      {historyKey && (
        <KeyHistoryModal apiKey={historyKey} onClose={() => setHistoryKey(null)} />
      )}
    </div>
  )
}
