'use client'

import { useState, useMemo, useOptimistic } from 'react'
import { useQuery } from '@tanstack/react-query'
import { keysQuery, resourceAuditQuery, keyMcpAccessQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { ApiKey, CreateKeyResponse, McpServerAccess } from '@/lib/types'
import { Plus, Trash2, BarChart2, RefreshCw, History, Key, ChevronLeft, ChevronRight, Server } from 'lucide-react'
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
import { StatusPill } from '@/components/status-pill'
import { KeyUsageModal } from '@/components/key-usage-modal'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDateOnly } from '@/lib/date'

function KeyStatusPills({ keys }: { keys: ApiKey[] }) {
  const { t } = useTranslation()
  const activeCount = useMemo(() => keys.filter(k => k.is_active).length, [keys])
  const inactiveCount = keys.length - activeCount
  return (
    <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap vds-mt-2">
      <StatusPill icon={<Key className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={keys.length} label={t('keys.registered')} />
      {activeCount > 0 && (
        <StatusPill
          icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-flex-shrink-0" />}
          count={activeCount} label={t('common.active')}
          className="vds-bg-success/10 vds-border-1 vds-border-success/30 vds-text-success"
        />
      )}
      {inactiveCount > 0 && (
        <StatusPill
          icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-flex-shrink-0" />}
          count={inactiveCount} label={t('common.inactive')}
          className="vds-bg-error/10 vds-border-1 vds-border-error/30 vds-text-error"
        />
      )}
    </div>
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
  const [tier, setTier] = useState<'free' | 'paid'>('paid')

  const mutation = useApiMutation(
    () => api.createKey({
      name: name.trim(),
      tenant_id: tenantId.trim(),
      rate_limit_rpm: rpm ? parseInt(rpm, 10) : undefined,
      rate_limit_tpm: tpm ? parseInt(tpm, 10) : undefined,
      tier,
    }),
    { invalidateKey: ['keys'], onSuccess: (data) => onCreated(data) },
  )

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle>{t('keys.createTitle')}</DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="key-name">{t('keys.keyName')} <span className="vds-text-destructive">*</span></Label>
            <Input
              id="key-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('keys.keyNamePlaceholder')}
            />
          </div>

          <div className="vds-space-y-1.5">
            <Label htmlFor="key-tenant">{t('keys.tenantId')}</Label>
            <Input
              id="key-tenant"
              value={tenantId}
              onChange={(e) => setTenantId(e.target.value)}
              placeholder={t('keys.tenantIdPlaceholder')}
            />
          </div>

          <div className="vds-space-y-1.5">
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

          <div className="vds-grid vds-grid-cols-2 vds-gap-3">
            <div className="vds-space-y-1.5">
              <Label htmlFor="key-rpm">{t('keys.rateLimitRpm')}</Label>
              <Input
                id="key-rpm"
                type="number"
                value={rpm}
                onChange={(e) => setRpm(e.target.value)}
                placeholder={t('keys.rateLimitPlaceholder')}
              />
            </div>
            <div className="vds-space-y-1.5">
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
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.unknownError')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate(undefined)} disabled={!name.trim() || mutation.isPending}>
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
      <DialogContent className="vds-max-w-lg" showClose={false}>
        <DialogHeader>
          <DialogTitle>{t('keys.createdTitle')}</DialogTitle>
        </DialogHeader>

        <div className="vds-rounded-lg vds-border-1 vds-border-warning/30 vds-bg-warning/10 vds-p-4 vds-text-warning vds-text-sm">
          {t('keys.createdWarning')}
        </div>

        <div className="vds-rounded-lg vds-bg-muted vds-p-3 vds-flex vds-items-center vds-gap-2">
          <code className="vds-flex-1 vds-font-mono vds-text-sm vds-text-success vds-break-all vds-select-all">{resp.key}</code>
          <CopyButton text={resp.key} />
        </div>

        <div className="vds-flex vds-items-center vds-gap-2">
          <Checkbox id="key-ack" checked={ack} onCheckedChange={(v) => setAck(v === true)} />
          <Label htmlFor="key-ack" className="vds-text-sm vds-cursor-pointer">{t('keys.keySavedAck')}</Label>
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
      <DialogContent className="vds-max-w-lg vds-max-h-[70vh] vds-flex vds-flex-col">
        <DialogHeader>
          <DialogTitle>{t('keys.historyTitle', { name: apiKey.name })}</DialogTitle>
        </DialogHeader>
        <div className="vds-flex-1 vds-overflow-y-auto vds-space-y-2 vds-min-h-0">
          {isLoading && <p className="vds-text-sm vds-text-dim">{t('common.loading')}</p>}
          {events && events.length === 0 && (
            <p className="vds-text-sm vds-text-dim">{t('common.empty')}</p>
          )}
          {events?.map((ev) => (
            <div key={`${ev.event_time}-${ev.account_id}-${ev.action}-${ev.resource_id}`} className="vds-rounded-lg vds-border-1 vds-px-3 vds-py-2 vds-text-sm vds-space-y-0.5">
              <div className="vds-flex vds-items-center vds-justify-between vds-gap-2">
                <Badge variant="outline" className="vds-text-[10px] vds-whitespace-nowrap">{ev.action}</Badge>
                <span className="vds-text-xs vds-text-dim">{fmtDateOnly(ev.event_time, tz)}</span>
              </div>
              <p className="vds-text-xs vds-text-dim">{ev.details}</p>
              <p className="vds-text-[10px] vds-text-dim/60">{ev.account_name}</p>
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

function KeyMcpAccessModal({ apiKey, onClose }: { apiKey: ApiKey; onClose: () => void }) {
  const { t } = useTranslation()

  // ── MCP cap points ────────────────────────────────────────────────────────
  const [capPoints, setCapPoints] = useState(String(apiKey.mcp_cap_points ?? 3))
  const capMutation = useApiMutation(
    (val: number) => api.patchKey(apiKey.id, { mcp_cap_points: val }),
    { invalidateKey: ['keys'] },
  )

  // ── Per-server top-k state ────────────────────────────────────────────────
  const [topKMap, setTopKMap] = useState<Record<string, string>>({})

  const { data: servers, isLoading, error } = useQuery(keyMcpAccessQuery(apiKey.id))

  const grantMutation = useApiMutation(
    ({ serverId, topK }: { serverId: string; topK: number | null }) =>
      api.grantKeyMcpAccess(apiKey.id, serverId, topK),
    { invalidateKey: ['key-mcp-access', apiKey.id] },
  )

  const revokeMutation = useApiMutation(
    (serverId: string) => api.revokeKeyMcpAccess(apiKey.id, serverId),
    { invalidateKey: ['key-mcp-access', apiKey.id] },
  )

  const isPending = grantMutation.isPending || revokeMutation.isPending

  function getTopK(s: McpServerAccess): number | null {
    const raw = topKMap[s.server_id]
    if (raw !== undefined) return raw === '' ? null : parseInt(raw, 10) || null
    return s.top_k
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-lg vds-max-h-[80vh] vds-flex vds-flex-col">
        <DialogHeader>
          <DialogTitle>{t('keys.mcpAccessTitle', { name: apiKey.name })}</DialogTitle>
        </DialogHeader>

        {/* MCP Cap Points */}
        <div className="vds-rounded-lg vds-border-1 vds-px-3 vds-py-2.5 vds-flex vds-items-center vds-gap-3">
          <Label className="vds-text-sm vds-flex-shrink-0">MCP Cap Points</Label>
          <Input
            type="number"
            min={0}
            max={10}
            value={capPoints}
            onChange={(e) => setCapPoints(e.target.value)}
            className="vds-h-7 vds-w-20 vds-text-xs"
          />
          <Button
            size="sm"
            variant="outline"
            className="vds-h-7 vds-text-xs"
            disabled={capMutation.isPending}
            onClick={() => {
              const val = Math.max(0, Math.min(10, parseInt(capPoints, 10) || 0))
              setCapPoints(String(val))
              capMutation.mutate(val)
            }}
          >
            {capMutation.isPending ? '…' : t('common.save', 'Save')}
          </Button>
          <span className="vds-text-xs vds-text-dim">0–10</span>
        </div>

        <p className="vds-text-sm vds-text-dim">{t('keys.mcpAccessDesc')}</p>
        <div className="vds-flex-1 vds-overflow-y-auto vds-space-y-2 vds-min-h-0">
          {isLoading && <p className="vds-text-sm vds-text-dim">{t('common.loading')}</p>}
          {error && <p className="vds-text-sm vds-text-destructive">{t('keys.mcpLoadError')}</p>}
          {servers && servers.length === 0 && (
            <p className="vds-text-sm vds-text-dim">{t('keys.mcpNoServers')}</p>
          )}
          {servers?.map((s: McpServerAccess) => (
            <div key={s.server_id} className="vds-flex vds-items-center vds-justify-between vds-rounded-lg vds-border-1 vds-px-3 vds-py-2 vds-gap-2">
              <div className="vds-flex vds-items-center vds-gap-2 vds-min-w-0 vds-flex-1">
                <Server className="vds-h-4 vds-w-4 vds-flex-shrink-0 vds-text-dim" />
                <div className="vds-min-w-0">
                  <p className="vds-text-sm vds-font-500 vds-truncate">{s.server_name}</p>
                  <p className="vds-text-xs vds-text-dim vds-font-mono">{s.slug}</p>
                </div>
              </div>
              <div className="vds-flex vds-items-center vds-gap-2 vds-flex-shrink-0">
                {/* Top-K input */}
                <div className="vds-flex vds-items-center vds-gap-1">
                  <Label className="vds-text-xs vds-text-dim vds-flex-shrink-0">Top-K</Label>
                  <Input
                    type="number"
                    min={1}
                    max={64}
                    placeholder="—"
                    value={topKMap[s.server_id] !== undefined ? topKMap[s.server_id] : (s.top_k ?? '')}
                    onChange={(e) => setTopKMap(prev => ({ ...prev, [s.server_id]: e.target.value }))}
                    className="vds-h-7 vds-w-16 vds-text-xs"
                  />
                </div>
                <Badge
                  variant="outline"
                  className={s.is_allowed
                    ? 'vds-bg-success/15 vds-text-success vds-border-success/30'
                    : 'vds-bg-muted vds-text-dim'}
                >
                  {s.is_allowed ? t('keys.mcpGranted') : t('keys.mcpNotGranted')}
                </Badge>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={isPending}
                  onClick={() => s.is_allowed
                    ? revokeMutation.mutate(s.server_id)
                    : grantMutation.mutate({ serverId: s.server_id, topK: getTopK(s) })
                  }
                >
                  {s.is_allowed ? t('keys.mcpRevoke') : t('keys.mcpGrant')}
                </Button>
              </div>
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

function KeyActiveSwitch({ keyId, isActive }: { keyId: string; isActive: boolean }) {
  const { t } = useTranslation()
  const [optimistic, setOptimistic] = useOptimistic(isActive, (_, v: boolean) => v)
  const mutation = useApiMutation(
    (vars: { id: string; is_active: boolean }) => api.toggleKeyActive(vars.id, vars.is_active),
    { invalidateKey: ['keys'] },
  )
  return (
    <Switch
      checked={optimistic}
      onCheckedChange={(checked) => { setOptimistic(checked); mutation.mutate({ id: keyId, is_active: checked }) }}
      aria-label={optimistic ? t('common.deactivate') : t('common.activate')}
    />
  )
}

export default function KeysPage() {
  usePageGuard('key_manage')
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [showCreate, setShowCreate] = useState(false)
  const [createdKey, setCreatedKey] = useState<CreateKeyResponse | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null)
  const [regenerateTarget, setRegenerateTarget] = useState<ApiKey | null>(null)
  const [usageKey, setUsageKey] = useState<ApiKey | null>(null)
  const [historyKey, setHistoryKey] = useState<ApiKey | null>(null)
  const [mcpAccessKey, setMcpAccessKey] = useState<ApiKey | null>(null)

  const { data: keysData, isLoading, error } = useQuery(keysQuery())
  const keys = keysData?.keys
  const [keyPage, setKeyPage] = useState(0)
  const KEY_PAGE_SIZE = 20

  const hasCreatedBy = keys?.some((k) => k.created_by)
  const keyTotalPages = keys ? Math.max(1, Math.ceil(keys.length / KEY_PAGE_SIZE)) : 0
  const keySafePage = Math.min(keyPage, Math.max(0, keyTotalPages - 1))
  const keyPageItems = useMemo(() => keys?.slice(keySafePage * KEY_PAGE_SIZE, (keySafePage + 1) * KEY_PAGE_SIZE) ?? [], [keys, keySafePage])

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteKey(id),
    { invalidateKey: ['keys'], onSuccess: () => setDeleteTarget(null) },
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
  }

  return (
    <div className="vds-space-y-8">
      <div>
        <div className="vds-flex vds-items-center vds-justify-between">
          <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('keys.title')}</h1>
          <Button onClick={() => setShowCreate(true)} className="vds-flex-shrink-0">
            <Plus className="vds-h-4 vds-w-4 vds-mr-2" />{t('keys.createKey')}
          </Button>
        </div>
        <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('keys.description')}</p>
        {keys ? (
          <KeyStatusPills keys={keys} />
        ) : (
          <p className="vds-text-sm vds-text-dim vds-mt-2 vds-animate-pulse">{t('common.loading')}</p>
        )}
      </div>

      {isLoading && (
        <div className="vds-flex vds-h-48 vds-items-center vds-justify-center vds-text-dim">
          {t('keys.loadingKeys')}
        </div>
      )}

      {error && (
        <Card className="vds-border-destructive/50 vds-bg-destructive/10">
          <CardContent className="vds-p-6 vds-text-destructive">
            <p className="vds-font-600">{t('keys.failedKeys')}</p>
            <p className="vds-text-sm vds-mt-1 vds-opacity-80">
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
                  <TableHead className="vds-whitespace-nowrap">{t('keys.name')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.prefix')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.tenant')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.tier')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.status')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.activeToggle')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('keys.rpmTpm')}</TableHead>
                  {hasCreatedBy && <TableHead className="vds-whitespace-nowrap">{t('keys.createdBy')}</TableHead>}
                  <TableHead className="vds-whitespace-nowrap">{t('keys.createdAt')}</TableHead>
                  <TableHead className="vds-text-right vds-whitespace-nowrap">{t('keys.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keyPageItems.map((key) => (
                  <TableRow key={key.id} className={!key.is_active ? 'vds-opacity-50' : ''}>
                    <TableCell className="vds-font-500">{key.name}</TableCell>
                    <TableCell className="vds-font-mono vds-text-xs">{key.key_prefix}</TableCell>
                    <TableCell className="vds-text-dim">{key.tenant_id}</TableCell>
                    <TableCell>
                      <Select
                        value={key.tier}
                        onValueChange={(v) =>
                          tierMutation.mutate({ id: key.id, tier: v as 'free' | 'paid' })
                        }
                        disabled={tierMutation.isPending}
                      >
                        <SelectTrigger className="vds-h-7 vds-w-24 vds-text-xs">
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
                        className={`vds-whitespace-nowrap ${
 key.is_active
 ? 'vds-bg-success-bg/15 vds-text-success vds-border-success/30'
 : 'vds-bg-muted vds-text-dim'
 }`}
                      >
                        {key.is_active ? t('common.active') : t('common.inactive')}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <KeyActiveSwitch keyId={key.id} isActive={key.is_active} />
                    </TableCell>
                    <TableCell className="vds-text-dim vds-text-xs vds-tabular-nums">
                      {key.rate_limit_rpm === 0 ? '∞' : key.rate_limit_rpm} /{' '}
                      {key.rate_limit_tpm === 0 ? '∞' : key.rate_limit_tpm}
                    </TableCell>
                    {hasCreatedBy && (
                      <TableCell className="vds-text-dim vds-text-xs">
                        {key.created_by ?? '—'}
                      </TableCell>
                    )}
                    <TableCell className="vds-text-dim vds-text-xs">
                      {fmtDateOnly(key.created_at, tz)}
                    </TableCell>
                    <TableCell className="vds-text-right">
                      <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.viewUsage')}
                          onClick={() => setUsageKey(key)}
                          title={t('keys.viewUsage')}
                          className="vds-text-dim vds-hover:text-primary"
                        >
                          <BarChart2 className="vds-h-4 vds-w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.viewHistory')}
                          onClick={() => setHistoryKey(key)}
                          title={t('keys.viewHistory')}
                          className="vds-text-dim vds-hover:text-primary"
                        >
                          <History className="vds-h-4 vds-w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.mcpAccess')}
                          onClick={() => setMcpAccessKey(key)}
                          title={t('keys.mcpAccess')}
                          className="vds-text-dim vds-hover:text-primary"
                        >
                          <Server className="vds-h-4 vds-w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.regenerateKey')}
                          onClick={() => setRegenerateTarget(key)}
                          title={t('keys.regenerateKey')}
                          className="vds-text-dim vds-hover:text-warning"
                        >
                          <RefreshCw className="vds-h-4 vds-w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('keys.deleteKey')}
                          onClick={() => setDeleteTarget(key)}
                          disabled={deleteMutation.isPending}
                          title={t('keys.deleteKey')}
                          className="vds-text-dim vds-hover:text-destructive"
                        >
                          <Trash2 className="vds-h-4 vds-w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </DataTable>
          )
      )}
      {keys && keys.length > 0 && keyTotalPages > 1 && (
        <div className="vds-flex vds-items-center vds-justify-end vds-gap-2">
          <span className="vds-text-xs vds-text-dim vds-tabular-nums">
            {keySafePage * KEY_PAGE_SIZE + 1}–{Math.min((keySafePage + 1) * KEY_PAGE_SIZE, keys.length)} / {keys.length}
          </span>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={keySafePage <= 0}
            onClick={() => setKeyPage(p => p - 1)}>
            <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
          </Button>
          <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={keySafePage >= keyTotalPages - 1}
            onClick={() => setKeyPage(p => p + 1)}>
            <ChevronRight className="vds-h-3.5 vds-w-3.5" />
          </Button>
        </div>
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

      {mcpAccessKey && (
        <KeyMcpAccessModal apiKey={mcpAccessKey} onClose={() => setMcpAccessKey(null)} />
      )}
    </div>
  )
}
