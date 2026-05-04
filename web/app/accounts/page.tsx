'use client'

import { useState, useMemo, useOptimistic, startTransition } from 'react'
import { useQuery } from '@tanstack/react-query'
import { accountsQuery, rolesQuery, accountSessionsQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Account, CreateAccountResponse, RoleSummary, SessionRecord } from '@/lib/types'
import { Plus, Trash2, Link, Shield, Settings2, Users, ChevronLeft, ChevronRight } from 'lucide-react'
import { CopyButton } from '@/components/copy-button'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Checkbox } from '@/components/ui/checkbox'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
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
import { useApiMutation } from '@/hooks/use-api-mutation'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime } from '@/lib/date'
import { hasPermission } from '@/lib/auth'

// ── Account active toggle with optimistic update ──────────────────────────────

function AccountActiveToggle({ account }: { account: Account }) {
  const { t } = useTranslation()
  const [optimistic, setOptimistic] = useOptimistic(account.is_active, (_, v: boolean) => v)
  const mutation = useApiMutation(
    (is_active: boolean) => api.setAccountActive(account.id, is_active),
    { invalidateKey: ['accounts'] },
  )
  return (
    <Switch
      checked={optimistic}
      onCheckedChange={(v) => startTransition(() => { setOptimistic(v); mutation.mutate(v) })}
      aria-label={optimistic ? t('common.deactivate') : t('common.activate')}
    />
  )
}

// ── Constants ─────────────────────────────────────────────────────────────────

const ALL_PERMISSIONS = [
  'dashboard_view', 'api_test', 'provider_manage',
  'key_manage', 'account_manage', 'audit_view', 'settings_manage',
  'role_manage', 'model_manage', 'mcp_manage',
] as const

// ── Sessions modal ────────────────────────────────────────────────────────────

function AccountSessionsModal({
  accountId,
  open,
  onClose,
}: {
  accountId: string
  open: boolean
  onClose: () => void
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()

  const { data: sessions = [], isLoading } = useQuery(accountSessionsQuery(accountId, open))

  const revokeMutation = useApiMutation(
    (sessionId: string) => api.revokeSession(sessionId),
    { invalidateKey: ['sessions', accountId] },
  )

  const revokeAllMutation = useApiMutation(
    (_: void) => api.revokeAllSessions(accountId),
    { invalidateKey: ['sessions', accountId] },
  )

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="vds-max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('accounts.sessions')}</DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-3 vds-py-1 vds-max-h-96 vds-overflow-y-auto">
          {isLoading ? (
            <p className="vds-text-sm vds-text-dim">{t('common.loading')}</p>
          ) : sessions.length === 0 ? (
            <p className="vds-text-sm vds-text-dim">{t('accounts.noSessions')}</p>
          ) : (
            sessions.map((s: SessionRecord) => (
              <div key={s.id} className="vds-flex vds-items-start vds-justify-between vds-gap-2 vds-rounded-md vds-border-1 vds-px-3 vds-py-2 vds-text-sm">
                <div className="vds-min-w-0 vds-flex-1 vds-space-y-0.5">
                  <div className="vds-font-mono vds-text-xs vds-text-dim vds-truncate">{s.ip_address ?? '—'}</div>
                  <div className="vds-text-xs vds-text-dim">
                    {t('accounts.lastUsed')}: {s.last_used_at ? fmtDatetime(s.last_used_at, tz) : t('common.never')}
                  </div>
                  <div className="vds-text-xs vds-text-dim">
                    {t('common.created')}: {fmtDatetime(s.created_at, tz)}
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="vds-h-7 vds-w-7 vds-flex-shrink-0 vds-text-destructive vds-hover:text-destructive"
                  aria-label={t('accounts.revokeSession')}
                  title={t('accounts.revokeSession')}
                  onClick={() => revokeMutation.mutate(s.id)}
                  disabled={revokeMutation.isPending}
                >
                  <Trash2 className="vds-h-3.5 vds-w-3.5" />
                </Button>
              </div>
            ))
          )}
        </div>
        <DialogFooter className="vds-gap-2">
          {sessions.length > 0 && (
            <Button
              variant="destructive"
              size="sm"
              onClick={() => revokeAllMutation.mutate()}
              disabled={revokeAllMutation.isPending}
            >
              {t('accounts.revokeAll')}
            </Button>
          )}
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Create account modal ──────────────────────────────────────────────────────

function CreateAccountModal({
  open,
  onClose,
  roles,
}: {
  open: boolean
  onClose: () => void
  roles: RoleSummary[]
}) {
  const { t } = useTranslation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')
  const [selectedRoleIds, setSelectedRoleIds] = useState<string[]>(() => {
    const viewer = roles.find(r => r.name === 'viewer')
    return viewer ? [viewer.id] : roles[0] ? [roles[0].id] : []
  })
  const [department, setDepartment] = useState('')
  const [position, setPosition] = useState('')
  const [created, setCreated] = useState<CreateAccountResponse | null>(null)

  function toggleRole(roleId: string) {
    setSelectedRoleIds(prev =>
      prev.includes(roleId) ? prev.filter(id => id !== roleId) : [...prev, roleId]
    )
  }

  const mutation = useApiMutation(
    (_: void) => api.createAccount({
      username, password, name,
      email: email || undefined,
      role_ids: selectedRoleIds,
      department: department || undefined,
      position: position || undefined,
    }),
    { invalidateKey: ['accounts'], onSuccess: (data) => setCreated(data) },
  )

  function handleClose() {
    setUsername(''); setPassword(''); setName(''); setEmail('')
    const viewer = roles.find(r => r.name === 'viewer')
    setSelectedRoleIds(viewer ? [viewer.id] : roles[0] ? [roles[0].id] : [])
    setDepartment(''); setPosition(''); setCreated(null)
    onClose()
  }

  if (created) {
    return (
      <Dialog open={open} onOpenChange={handleClose}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('accounts.accountCreated')}</DialogTitle>
          </DialogHeader>
          <div className="vds-space-y-3 vds-py-2">
            <div className="vds-rounded-lg vds-border-1 vds-border-warning/30 vds-bg-warning/10 vds-p-4 vds-text-warning vds-text-sm">
              {t('accounts.saveKeyWarning')}
            </div>
            <div className="vds-flex vds-items-center vds-gap-2 vds-rounded-md vds-border-1 vds-bg-muted vds-px-3 vds-py-2">
              <code className="vds-flex-1 vds-font-mono vds-text-xs vds-break-all vds-select-all">{created.test_api_key}</code>
              <CopyButton text={created.test_api_key} />
            </div>
          </div>
          <DialogFooter>
            <Button onClick={handleClose}>{t('common.done')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('accounts.createAccount')}</DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-3 vds-py-1">
          <div className="vds-grid vds-grid-cols-2 vds-gap-3">
            <div className="vds-space-y-1.5">
              <Label htmlFor="create-account-username">{t('accounts.username')}</Label>
              <Input id="create-account-username" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" />
            </div>
            <div className="vds-space-y-1.5">
              <Label htmlFor="create-account-name">{t('accounts.fullName')}</Label>
              <Input id="create-account-name" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
          </div>
          <div className="vds-space-y-1.5">
            <Label htmlFor="create-account-password">{t('accounts.password')}</Label>
            <Input id="create-account-password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
          </div>
          <div className="vds-grid vds-grid-cols-2 vds-gap-3">
            <div className="vds-space-y-1.5">
              <Label htmlFor="create-account-email">{t('accounts.email')}</Label>
              <Input id="create-account-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
            </div>
            <div className="vds-space-y-1.5">
              <Label>{t('accounts.role')}</Label>
              <div className="vds-space-y-1.5 vds-rounded-md vds-border-1 vds-p-2 vds-max-h-32 vds-overflow-y-auto">
                {roles.map(r => (
                  <label key={r.id} className="vds-flex vds-items-center vds-gap-2 vds-text-sm vds-cursor-pointer">
                    <Checkbox
                      checked={selectedRoleIds.includes(r.id)}
                      onCheckedChange={() => toggleRole(r.id)}
                    />
                    <span>{r.name}</span>
                    {r.is_system && <Badge variant="secondary" className="vds-text-[10px] vds-h-4 vds-px-1 vds-whitespace-nowrap">{t('roles.system')}</Badge>}
                  </label>
                ))}
              </div>
            </div>
          </div>
          <div className="vds-grid vds-grid-cols-2 vds-gap-3">
            <div className="vds-space-y-1.5">
              <Label htmlFor="create-account-department">{t('accounts.department')}</Label>
              <Input id="create-account-department" value={department} onChange={(e) => setDepartment(e.target.value)} />
            </div>
            <div className="vds-space-y-1.5">
              <Label htmlFor="create-account-position">{t('accounts.position')}</Label>
              <Input id="create-account-position" value={position} onChange={(e) => setPosition(e.target.value)} />
            </div>
          </div>
          {mutation.isError && (
            <p className="vds-text-sm vds-text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : t('accounts.createFailed')}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={handleClose}>{t('common.cancel')}</Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !username || !password || !name || selectedRoleIds.length === 0}
          >
            {mutation.isPending ? t('accounts.creating') : t('common.create')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Role editor modal ─────────────────────────────────────────────────────────

function RoleEditorModal({
  open,
  onClose,
  role,
}: {
  open: boolean
  onClose: () => void
  role?: RoleSummary | null
}) {
  const { t } = useTranslation()
  const isNew = !role
  const isSystem = role?.is_system ?? false
  const [name, setName] = useState(role?.name ?? '')
  const [perms, setPerms] = useState<string[]>(role?.permissions ?? [])

  const mutation = useApiMutation(
    async (_: void) => {
      // Menu visibility derives from permissions (see lib/route-permissions.ts).
      if (isNew) {
        await api.createRole({ name, permissions: perms })
      } else if (role) {
        await api.updateRole(role.id, { name: name !== role.name ? name : undefined, permissions: perms })
      }
    },
    { invalidateKey: ['roles'], onSuccess: () => onClose() },
  )

  function togglePerm(p: string) {
    setPerms(prev => prev.includes(p) ? prev.filter(x => x !== p) : [...prev, p])
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="vds-max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {isNew ? t('roles.createRole') : t('roles.editRole')}
            {isSystem && <Badge variant="secondary" className="vds-ml-2 vds-whitespace-nowrap">{t('roles.system')}</Badge>}
          </DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-4 vds-py-1">
          <div className="vds-space-y-1.5">
            <Label htmlFor="role-name">{t('roles.roleName')}</Label>
            <Input id="role-name" value={name} onChange={e => setName(e.target.value)} disabled={isSystem} />
          </div>

          {/* Permissions section */}
          <div className="vds-space-y-1.5">
            <Label>{t('roles.permissions')}</Label>
            <div className="vds-grid vds-grid-cols-2 vds-gap-2 vds-rounded-md vds-border-1 vds-p-3">
              {ALL_PERMISSIONS.map(p => (
                <label key={p} className="vds-flex vds-items-center vds-gap-2 vds-text-sm vds-cursor-pointer">
                  <Checkbox
                    checked={perms.includes(p)}
                    onCheckedChange={() => togglePerm(p)}
                    disabled={isSystem}
                  />
                  <span>{t(`roles.perm.${p}` as Parameters<typeof t>[0])}</span>
                </label>
              ))}
            </div>
          </div>

          {mutation.isError && (
            <p className="vds-text-sm vds-text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          {!isSystem && (
            <Button onClick={() => mutation.mutate()} disabled={mutation.isPending || !name}>
              {mutation.isPending ? t('common.saving') : t('common.save')}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ── Edit account roles modal ──────────────────────────────────────────────────

function EditRolesModal({
  open,
  onClose,
  account,
  roles,
}: {
  open: boolean
  onClose: () => void
  account: Account
  roles: RoleSummary[]
}) {
  const { t } = useTranslation()
  const [selectedRoleIds, setSelectedRoleIds] = useState<string[]>(
    account.roles.map(r => r.id)
  )

  function toggleRole(roleId: string) {
    setSelectedRoleIds(prev =>
      prev.includes(roleId) ? prev.filter(id => id !== roleId) : [...prev, roleId]
    )
  }

  const mutation = useApiMutation(
    (_: void) => api.updateAccount(account.id, { role_ids: selectedRoleIds }),
    { invalidateKey: ['accounts'], onSuccess: () => onClose() },
  )

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="vds-max-w-sm">
        <DialogHeader>
          <DialogTitle>{t('roles.editRole')} — {account.username}</DialogTitle>
        </DialogHeader>
        <div className="vds-space-y-2 vds-py-1">
          {roles.map(r => (
            <label key={r.id} className="vds-flex vds-items-center vds-gap-2.5 vds-rounded-md vds-border-1 vds-px-3 vds-py-2 vds-cursor-pointer vds-hover:bg-hover/50 vds-transition-colors">
              <Checkbox
                checked={selectedRoleIds.includes(r.id)}
                onCheckedChange={() => toggleRole(r.id)}
              />
              <div className="vds-flex-1 vds-min-w-0">
                <div className="vds-text-sm vds-font-500 vds-flex vds-items-center vds-gap-1.5">
                  {r.name}
                  {r.is_system && <Badge variant="secondary" className="vds-text-[10px] vds-h-4 vds-px-1 vds-whitespace-nowrap">{t('roles.system')}</Badge>}
                </div>
                <div className="vds-text-xs vds-text-dim">
                  {t('roles.permissionCount', { count: r.permissions.length })}
                </div>
              </div>
            </label>
          ))}
          {mutation.isError && (
            <p className="vds-text-sm vds-text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || selectedRoleIds.length === 0}
          >
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function AccountStatusPills({ accounts }: { accounts: Account[] }) {
  const { t } = useTranslation()
  const activeCount = useMemo(() => accounts.filter(a => a.is_active).length, [accounts])
  return (
    <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap vds-mt-2">
      <StatusPill icon={<Users className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={accounts.length} label={t('accounts.registered')} />
      {activeCount > 0 && (
        <StatusPill
          icon={<span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-flex-shrink-0" />}
          count={activeCount} label={t('common.active')}
          className="vds-bg-success/10 vds-border-1 vds-border-success/30 vds-text-success"
        />
      )}
    </div>
  )
}

// ── Roles tab ─────────────────────────────────────────────────────────────────

function RolesTab() {
  const { t } = useTranslation()
  const [editRole, setEditRole] = useState<RoleSummary | null | undefined>(undefined)
  const [deleteTarget, setDeleteTarget] = useState<RoleSummary | null>(null)

  const { data: roles = [], isLoading, isError } = useQuery(rolesQuery)

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteRole(id),
    { invalidateKey: ['roles'], onSuccess: () => setDeleteTarget(null) },
  )

  return (
    <div className="vds-space-y-4">
      <div className="vds-flex vds-items-center vds-justify-between">
        <div>
          <h2 className="vds-text-lg vds-font-600">{t('roles.title')}</h2>
          <p className="vds-text-sm vds-text-dim">{t('roles.description')}</p>
        </div>
        <Button size="sm" onClick={() => setEditRole(null)}>
          <Plus className="vds-h-4 vds-w-4 vds-mr-1.5" />
          {t('roles.createRole')}
        </Button>
      </div>

      {editRole !== undefined && (
        <RoleEditorModal
          open
          onClose={() => setEditRole(undefined)}
          role={editRole}
        />
      )}

      {isLoading ? (
        <p className="vds-text-sm vds-text-dim">{t('common.loading')}</p>
      ) : isError ? (
        <p className="vds-text-sm vds-text-destructive">{t('common.error')}</p>
      ) : roles.length === 0 ? (
        <DataTableEmpty>{t('roles.noRoles')}</DataTableEmpty>
      ) : (
        <div className="vds-grid vds-gap-3 vds-sm:grid-cols-2">
          {roles.map((r: RoleSummary) => (
            <Card key={r.id} className="vds-relative">
              <CardHeader className="vds-pb-2">
                <CardTitle className="vds-text-sm vds-font-500 vds-flex vds-items-center vds-gap-2">
                  {r.name}
                  {r.is_system && <Badge variant="secondary" className="vds-text-[10px] vds-h-4 vds-px-1 vds-whitespace-nowrap">{t('roles.system')}</Badge>}
                </CardTitle>
              </CardHeader>
              <CardContent className="vds-space-y-2 vds-pb-3">
                <div className="vds-flex vds-flex-wrap vds-gap-1">
                  {r.permissions.map(p => (
                    <Badge key={p} variant="outline" className="vds-text-[10px] vds-font-400 vds-whitespace-nowrap">
                      {t(`roles.perm.${p}` as Parameters<typeof t>[0])}
                    </Badge>
                  ))}
                </div>
                <div className="vds-flex vds-items-center vds-justify-between vds-text-xs vds-text-dim">
                  <span>{t('roles.assignedUsers', { count: r.account_count })}</span>
                  {!r.is_system && (
                    <div className="vds-flex vds-items-center vds-gap-0.5">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="vds-h-6 vds-w-6"
                        aria-label={t('common.edit')}
                        title={t('common.edit')}
                        onClick={() => setEditRole(r)}
                      >
                        <Settings2 className="vds-h-3 vds-w-3" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="vds-h-6 vds-w-6 vds-text-destructive vds-hover:text-destructive"
                        aria-label={t('common.delete')}
                        title={t('common.delete')}
                        onClick={() => setDeleteTarget(r)}
                        disabled={r.account_count > 0}
                      >
                        <Trash2 className="vds-h-3 vds-w-3" />
                      </Button>
                    </div>
                  )}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {deleteTarget && (
        <ConfirmDialog
          open
          title={t('common.delete')}
          description={t('roles.deleteConfirm', { name: deleteTarget.name })}
          confirmLabel={deleteMutation.isPending ? t('common.deleting') : t('common.delete')}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isLoading={deleteMutation.isPending}
        />
      )}
    </div>
  )
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function AccountsPage() {
  usePageGuard('account_manage')
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const canManageRoles = hasPermission('role_manage')
  const [tab, setTab] = useState<'accounts' | 'roles'>('accounts')
  const [showCreate, setShowCreate] = useState(false)
  const [resetToken, setResetToken] = useState<string | null>(null)
  const [sessionsAccountId, setSessionsAccountId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Account | null>(null)
  const [editRolesTarget, setEditRolesTarget] = useState<Account | null>(null)

  const { data: accountsData, isLoading, isError } = useQuery(accountsQuery())
  const accounts = accountsData?.accounts ?? []
  const { data: roles = [] } = useQuery(rolesQuery)
  const [acctPage, setAcctPage] = useState(0)
  const ACCT_PAGE_SIZE = 20
  const acctTotalPages = Math.max(1, Math.ceil(accounts.length / ACCT_PAGE_SIZE))
  const acctSafePage = Math.min(acctPage, Math.max(0, acctTotalPages - 1))
  const acctPageItems = useMemo(() => accounts.slice(acctSafePage * ACCT_PAGE_SIZE, (acctSafePage + 1) * ACCT_PAGE_SIZE), [accounts, acctSafePage])

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteAccount(id),
    { invalidateKey: ['accounts'], onSuccess: () => setDeleteTarget(null) },
  )

  const resetMutation = useApiMutation(
    (id: string) => api.createResetLink(id),
    { onSuccess: (data) => setResetToken(data.token) },
  )

  return (
    <div className="vds-flex vds-flex-col vds-gap-6 vds-p-6 vds-max-w-5xl vds-mx-auto">
      {/* Tab switcher (only for super users) */}
      {canManageRoles && (
        <div className="vds-flex vds-gap-1 vds-border-b-1 vds-border-subtle">
          <button
            type="button"
            className={`vds-px-4 vds-py-2 vds-text-sm vds-font-500 vds-border-b-2 vds-transition-colors ${
 tab === 'accounts' ? 'vds-border-primary vds-text-primary' : 'vds-border-transparent vds-text-dim vds-hover:text-primary'
 }`}
            onClick={() => setTab('accounts')}
          >
            {t('accounts.title')}
          </button>
          <button
            type="button"
            className={`vds-px-4 vds-py-2 vds-text-sm vds-font-500 vds-border-b-2 vds-transition-colors ${
 tab === 'roles' ? 'vds-border-primary vds-text-primary' : 'vds-border-transparent vds-text-dim vds-hover:text-primary'
 }`}
            onClick={() => setTab('roles')}
          >
            {t('roles.title')}
          </button>
        </div>
      )}

      {/* Roles tab */}
      {tab === 'roles' && canManageRoles && <RolesTab />}

      {/* Accounts tab */}
      {tab === 'accounts' && (
        <>
          <div>
            <div className="vds-flex vds-items-center vds-justify-between">
              <h1 className="vds-text-xl vds-font-600">{t('accounts.title')}</h1>
              <Button size="sm" onClick={() => setShowCreate(true)} className="vds-flex-shrink-0">
                <Plus className="vds-h-4 vds-w-4 vds-mr-1.5" />{t('accounts.createAccount')}
              </Button>
            </div>
            <p className="vds-text-sm vds-text-dim vds-mt-0.5">{t('accounts.description')}</p>
            {accounts.length > 0 && (
              <AccountStatusPills accounts={accounts} />
            )}
          </div>

          <CreateAccountModal open={showCreate} onClose={() => setShowCreate(false)} roles={roles} />
          {sessionsAccountId && (
            <AccountSessionsModal
              accountId={sessionsAccountId}
              open={!!sessionsAccountId}
              onClose={() => setSessionsAccountId(null)}
            />
          )}
          {editRolesTarget && (
            <EditRolesModal
              open={!!editRolesTarget}
              onClose={() => setEditRolesTarget(null)}
              account={editRolesTarget}
              roles={roles}
            />
          )}

          {/* Reset token display */}
          {resetToken && (
            <Dialog open onOpenChange={() => setResetToken(null)}>
              <DialogContent className="vds-max-w-lg">
                <DialogHeader>
                  <DialogTitle>{t('accounts.resetLink')}</DialogTitle>
                </DialogHeader>
                <div className="vds-rounded-lg vds-border-1 vds-border-warning/30 vds-bg-warning/10 vds-p-4 vds-text-warning vds-text-sm">
                  {t('accounts.tokenWarning')}
                </div>
                <div className="vds-rounded-lg vds-bg-muted vds-p-3 vds-flex vds-items-center vds-gap-2">
                  <code className="vds-flex-1 vds-font-mono vds-text-xs vds-break-all vds-select-all">{resetToken}</code>
                  <CopyButton text={resetToken} />
                </div>
                <DialogFooter>
                  <Button onClick={() => setResetToken(null)}>{t('common.done')}</Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          )}

          {isLoading ? (
            <p className="vds-text-sm vds-text-dim">{t('common.loading')}</p>
          ) : isError ? (
            <p className="vds-text-sm vds-text-destructive">{t('common.error')}</p>
          ) : accounts.length === 0 ? (
            <DataTableEmpty>{t('accounts.noAccounts')}</DataTableEmpty>
          ) : (
            <DataTable minWidth="700px">
              <TableHeader>
                <TableRow>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.username')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.name')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.role')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.department')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.status')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.lastLogin')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('accounts.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {acctPageItems.map((a: Account) => (
                  <TableRow key={a.id}>
                    <TableCell className="vds-font-mono vds-text-xs">{a.username}</TableCell>
                    <TableCell>{a.name}</TableCell>
                    <TableCell>
                      <div className="vds-flex vds-flex-wrap vds-gap-1">
                        {a.roles.map(r => (
                          <Badge key={r.id} variant={r.name === 'super' ? 'default' : 'secondary'} className="vds-whitespace-nowrap">
                            {r.name}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="vds-text-dim vds-text-sm">{a.department ?? '—'}</TableCell>
                    <TableCell>
                      <AccountActiveToggle account={a} />
                    </TableCell>
                    <TableCell className="vds-text-xs vds-text-dim">
                      {a.last_login_at ? fmtDatetime(a.last_login_at, tz) : t('common.never')}
                    </TableCell>
                    <TableCell>
                      <div className="vds-flex vds-items-center vds-gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="vds-h-7 vds-w-7"
                          aria-label={t('roles.editRole')}
                          title={t('roles.editRole')}
                          onClick={() => setEditRolesTarget(a)}
                        >
                          <Settings2 className="vds-h-3.5 vds-w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="vds-h-7 vds-w-7"
                          aria-label={t('accounts.sessions')}
                          title={t('accounts.sessions')}
                          onClick={() => setSessionsAccountId(a.id)}
                        >
                          <Shield className="vds-h-3.5 vds-w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="vds-h-7 vds-w-7"
                          aria-label={t('accounts.resetLink')}
                          title={t('accounts.resetLink')}
                          onClick={() => resetMutation.mutate(a.id)}
                        >
                          <Link className="vds-h-3.5 vds-w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="vds-h-7 vds-w-7 vds-text-destructive vds-hover:text-destructive"
                          aria-label={t('common.delete')}
                          title={t('common.delete')}
                          onClick={() => setDeleteTarget(a)}
                        >
                          <Trash2 className="vds-h-3.5 vds-w-3.5" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </DataTable>
          )}
          {accounts.length > 0 && acctTotalPages > 1 && (
            <div className="vds-flex vds-items-center vds-justify-end vds-gap-2">
              <span className="vds-text-xs vds-text-dim vds-tabular-nums">
                {acctSafePage * ACCT_PAGE_SIZE + 1}–{Math.min((acctSafePage + 1) * ACCT_PAGE_SIZE, accounts.length)} / {accounts.length}
              </span>
              <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={acctSafePage <= 0}
                onClick={() => setAcctPage(p => p - 1)}>
                <ChevronLeft className="vds-h-3.5 vds-w-3.5" />
              </Button>
              <Button variant="outline" size="icon" className="vds-h-7 vds-w-7" disabled={acctSafePage >= acctTotalPages - 1}
                onClick={() => setAcctPage(p => p + 1)}>
                <ChevronRight className="vds-h-3.5 vds-w-3.5" />
              </Button>
            </div>
          )}

          {deleteTarget && (
            <ConfirmDialog
              open
              title={t('common.delete')}
              description={t('accounts.deleteConfirm', { name: deleteTarget.username })}
              confirmLabel={deleteMutation.isPending ? t('common.deleting') : t('common.delete')}
              onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
              onClose={() => setDeleteTarget(null)}
              isLoading={deleteMutation.isPending}
            />
          )}
        </>
      )}
    </div>
  )
}
