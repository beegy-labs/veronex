'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { accountsQuery, rolesQuery, accountSessionsQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Account, CreateAccountResponse, RoleSummary, SessionRecord } from '@/lib/types'
import { Plus, Trash2, Link, Shield, Settings2 } from 'lucide-react'
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
import { useApiMutation } from '@/hooks/use-api-mutation'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime } from '@/lib/date'
import { hasPermission } from '@/lib/auth'

// ── Constants ─────────────────────────────────────────────────────────────────

const ALL_PERMISSIONS = [
  'dashboard_view', 'api_test', 'provider_manage',
  'key_manage', 'account_manage', 'audit_view', 'settings_manage',
  'role_manage',
] as const

const ALL_MENUS = [
  'dashboard', 'flow', 'jobs', 'performance', 'usage', 'test',
  'providers', 'servers', 'keys', 'accounts', 'audit', 'api_docs',
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
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('accounts.sessions')}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 py-1 max-h-96 overflow-y-auto">
          {isLoading ? (
            <p className="text-sm text-muted-foreground">{t('common.loading')}</p>
          ) : sessions.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('accounts.noSessions')}</p>
          ) : (
            sessions.map((s: SessionRecord) => (
              <div key={s.id} className="flex items-start justify-between gap-2 rounded-md border px-3 py-2 text-sm">
                <div className="min-w-0 flex-1 space-y-0.5">
                  <div className="font-mono text-xs text-muted-foreground truncate">{s.ip_address ?? '—'}</div>
                  <div className="text-xs text-muted-foreground">
                    {t('accounts.lastUsed')}: {s.last_used_at ? fmtDatetime(s.last_used_at, tz) : t('common.never')}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t('common.created')}: {fmtDatetime(s.created_at, tz)}
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 text-destructive hover:text-destructive"
                  aria-label={t('accounts.revokeSession')}
                  title={t('accounts.revokeSession')}
                  onClick={() => revokeMutation.mutate(s.id)}
                  disabled={revokeMutation.isPending}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))
          )}
        </div>
        <DialogFooter className="gap-2">
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
          <div className="space-y-3 py-2">
            <div className="rounded-lg border border-status-warning/30 bg-status-warning/10 p-4 text-status-warning-fg text-sm">
              {t('accounts.saveKeyWarning')}
            </div>
            <div className="flex items-center gap-2 rounded-md border bg-muted px-3 py-2">
              <code className="flex-1 font-mono text-xs break-all select-all">{created.test_api_key}</code>
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
        <div className="space-y-3 py-1">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="create-account-username">{t('accounts.username')}</Label>
              <Input id="create-account-username" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="create-account-name">{t('accounts.fullName')}</Label>
              <Input id="create-account-name" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-account-password">{t('accounts.password')}</Label>
            <Input id="create-account-password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="create-account-email">{t('accounts.email')}</Label>
              <Input id="create-account-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>{t('accounts.role')}</Label>
              <div className="space-y-1.5 rounded-md border p-2 max-h-32 overflow-y-auto">
                {roles.map(r => (
                  <label key={r.id} className="flex items-center gap-2 text-sm cursor-pointer">
                    <Checkbox
                      checked={selectedRoleIds.includes(r.id)}
                      onCheckedChange={() => toggleRole(r.id)}
                    />
                    <span>{r.name}</span>
                    {r.is_system && <Badge variant="secondary" className="text-[10px] h-4 px-1 whitespace-nowrap">{t('roles.system')}</Badge>}
                  </label>
                ))}
              </div>
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="create-account-department">{t('accounts.department')}</Label>
              <Input id="create-account-department" value={department} onChange={(e) => setDepartment(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="create-account-position">{t('accounts.position')}</Label>
              <Input id="create-account-position" value={position} onChange={(e) => setPosition(e.target.value)} />
            </div>
          </div>
          {mutation.isError && (
            <p className="text-sm text-destructive">
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
  const [menus, setMenus] = useState<string[]>(role?.menus ?? [])

  const mutation = useApiMutation(
    async (_: void) => {
      if (isNew) {
        await api.createRole({ name, permissions: perms, menus })
      } else if (role) {
        await api.updateRole(role.id, { name: name !== role.name ? name : undefined, permissions: perms, menus })
      }
    },
    { invalidateKey: ['roles'], onSuccess: () => onClose() },
  )

  function togglePerm(p: string) {
    setPerms(prev => prev.includes(p) ? prev.filter(x => x !== p) : [...prev, p])
  }
  function toggleMenu(m: string) {
    setMenus(prev => prev.includes(m) ? prev.filter(x => x !== m) : [...prev, m])
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {isNew ? t('roles.createRole') : t('roles.editRole')}
            {isSystem && <Badge variant="secondary" className="ml-2 whitespace-nowrap">{t('roles.system')}</Badge>}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-4 py-1">
          <div className="space-y-1.5">
            <Label htmlFor="role-name">{t('roles.roleName')}</Label>
            <Input id="role-name" value={name} onChange={e => setName(e.target.value)} disabled={isSystem} />
          </div>

          {/* Permissions section */}
          <div className="space-y-1.5">
            <Label>{t('roles.permissions')}</Label>
            <div className="grid grid-cols-2 gap-2 rounded-md border p-3">
              {ALL_PERMISSIONS.map(p => (
                <label key={p} className="flex items-center gap-2 text-sm cursor-pointer">
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

          {/* Menus section */}
          <div className="space-y-1.5">
            <Label>{t('roles.menus')}</Label>
            <div className="grid grid-cols-3 gap-2 rounded-md border p-3">
              {ALL_MENUS.map(m => (
                <label key={m} className="flex items-center gap-2 text-sm cursor-pointer">
                  <Checkbox
                    checked={menus.includes(m)}
                    onCheckedChange={() => toggleMenu(m)}
                    disabled={isSystem}
                  />
                  <span>{t(`roles.menu.${m}` as Parameters<typeof t>[0])}</span>
                </label>
              ))}
            </div>
          </div>

          {mutation.isError && (
            <p className="text-sm text-destructive">
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
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t('roles.editRole')} — {account.username}</DialogTitle>
        </DialogHeader>
        <div className="space-y-2 py-1">
          {roles.map(r => (
            <label key={r.id} className="flex items-center gap-2.5 rounded-md border px-3 py-2 cursor-pointer hover:bg-accent/50 transition-colors">
              <Checkbox
                checked={selectedRoleIds.includes(r.id)}
                onCheckedChange={() => toggleRole(r.id)}
              />
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium flex items-center gap-1.5">
                  {r.name}
                  {r.is_system && <Badge variant="secondary" className="text-[10px] h-4 px-1 whitespace-nowrap">{t('roles.system')}</Badge>}
                </div>
                <div className="text-xs text-muted-foreground">
                  {t('roles.permissionCount', { count: r.permissions.length })} · {t('roles.menuCount', { count: r.menus.length })}
                </div>
              </div>
            </label>
          ))}
          {mutation.isError && (
            <p className="text-sm text-destructive">
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
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">{t('roles.title')}</h2>
          <p className="text-sm text-muted-foreground">{t('roles.description')}</p>
        </div>
        <Button size="sm" onClick={() => setEditRole(null)}>
          <Plus className="h-4 w-4 mr-1.5" />
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
        <p className="text-sm text-muted-foreground">{t('common.loading')}</p>
      ) : isError ? (
        <p className="text-sm text-destructive">{t('common.error')}</p>
      ) : roles.length === 0 ? (
        <DataTableEmpty>{t('roles.noRoles')}</DataTableEmpty>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2">
          {roles.map((r: RoleSummary) => (
            <Card key={r.id} className="relative">
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium flex items-center gap-2">
                  {r.name}
                  {r.is_system && <Badge variant="secondary" className="text-[10px] h-4 px-1 whitespace-nowrap">{t('roles.system')}</Badge>}
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-2 pb-3">
                <div className="flex flex-wrap gap-1">
                  {r.permissions.map(p => (
                    <Badge key={p} variant="outline" className="text-[10px] font-normal whitespace-nowrap">
                      {t(`roles.perm.${p}` as Parameters<typeof t>[0])}
                    </Badge>
                  ))}
                </div>
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>{t('roles.menuCount', { count: r.menus.length })} · {t('roles.assignedUsers', { count: r.account_count })}</span>
                  {!r.is_system && (
                    <div className="flex items-center gap-0.5">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6"
                        aria-label={t('common.edit')}
                        title={t('common.edit')}
                        onClick={() => setEditRole(r)}
                      >
                        <Settings2 className="h-3 w-3" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 text-destructive hover:text-destructive"
                        aria-label={t('common.delete')}
                        title={t('common.delete')}
                        onClick={() => setDeleteTarget(r)}
                        disabled={r.account_count > 0}
                      >
                        <Trash2 className="h-3 w-3" />
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
  usePageGuard('accounts')
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const canManageRoles = hasPermission('role_manage')
  const [tab, setTab] = useState<'accounts' | 'roles'>('accounts')
  const [showCreate, setShowCreate] = useState(false)
  const [resetToken, setResetToken] = useState<string | null>(null)
  const [sessionsAccountId, setSessionsAccountId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Account | null>(null)
  const [editRolesTarget, setEditRolesTarget] = useState<Account | null>(null)

  const { data: accounts = [], isLoading, isError } = useQuery(accountsQuery)
  const { data: roles = [] } = useQuery(rolesQuery)

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteAccount(id),
    { invalidateKey: ['accounts'], onSuccess: () => setDeleteTarget(null) },
  )

  const activeMutation = useApiMutation(
    (vars: { id: string; is_active: boolean }) => api.setAccountActive(vars.id, vars.is_active),
    { invalidateKey: ['accounts'] },
  )

  const resetMutation = useApiMutation(
    (id: string) => api.createResetLink(id),
    { onSuccess: (data) => setResetToken(data.token) },
  )

  return (
    <div className="flex flex-col gap-6 p-6 max-w-5xl mx-auto">
      {/* Tab switcher (only for super users) */}
      {canManageRoles && (
        <div className="flex gap-1 border-b border-border">
          <button
            type="button"
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === 'accounts' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            onClick={() => setTab('accounts')}
          >
            {t('accounts.title')}
          </button>
          <button
            type="button"
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === 'roles' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'
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
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-xl font-semibold">{t('accounts.title')}</h1>
              <p className="text-sm text-muted-foreground mt-0.5">{t('accounts.description')}</p>
            </div>
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus className="h-4 w-4 mr-1.5" />
              {t('accounts.createAccount')}
            </Button>
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
              <DialogContent className="max-w-lg">
                <DialogHeader>
                  <DialogTitle>{t('accounts.resetLink')}</DialogTitle>
                </DialogHeader>
                <div className="rounded-lg border border-status-warning/30 bg-status-warning/10 p-4 text-status-warning-fg text-sm">
                  {t('accounts.tokenWarning')}
                </div>
                <div className="rounded-lg bg-muted p-3 flex items-center gap-2">
                  <code className="flex-1 font-mono text-xs break-all select-all">{resetToken}</code>
                  <CopyButton text={resetToken} />
                </div>
                <DialogFooter>
                  <Button onClick={() => setResetToken(null)}>{t('common.done')}</Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          )}

          {isLoading ? (
            <p className="text-sm text-muted-foreground">{t('common.loading')}</p>
          ) : isError ? (
            <p className="text-sm text-destructive">{t('common.error')}</p>
          ) : accounts.length === 0 ? (
            <DataTableEmpty>{t('accounts.noAccounts')}</DataTableEmpty>
          ) : (
            <DataTable minWidth="700px">
              <TableHeader>
                <TableRow>
                  <TableHead className="whitespace-nowrap">{t('accounts.username')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.name')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.role')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.department')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.status')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.lastLogin')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('accounts.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {accounts.map((a: Account) => (
                  <TableRow key={a.id}>
                    <TableCell className="font-mono text-xs">{a.username}</TableCell>
                    <TableCell>{a.name}</TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {a.roles.map(r => (
                          <Badge key={r.id} variant={r.name === 'super' ? 'default' : 'secondary'}>
                            {r.name}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="text-muted-foreground text-sm">{a.department ?? '—'}</TableCell>
                    <TableCell>
                      <Switch
                        checked={a.is_active}
                        onCheckedChange={(v) => activeMutation.mutate({ id: a.id, is_active: v })}
                        aria-label={a.is_active ? t('common.deactivate') : t('common.activate')}
                      />
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {a.last_login_at ? fmtDatetime(a.last_login_at, tz) : t('common.never')}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          aria-label={t('roles.editRole')}
                          title={t('roles.editRole')}
                          onClick={() => setEditRolesTarget(a)}
                        >
                          <Settings2 className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          aria-label={t('accounts.sessions')}
                          title={t('accounts.sessions')}
                          onClick={() => setSessionsAccountId(a.id)}
                        >
                          <Shield className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          aria-label={t('accounts.resetLink')}
                          title={t('accounts.resetLink')}
                          onClick={() => resetMutation.mutate(a.id)}
                        >
                          <Link className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 text-destructive hover:text-destructive"
                          aria-label={t('common.delete')}
                          title={t('common.delete')}
                          onClick={() => setDeleteTarget(a)}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </DataTable>
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
