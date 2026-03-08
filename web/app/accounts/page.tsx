'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { accountsQuery, accountSessionsQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Account, CreateAccountResponse, SessionRecord } from '@/lib/types'
import { Plus, Trash2, Link, Shield } from 'lucide-react'
import { CopyButton } from '@/components/copy-button'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
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
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime } from '@/lib/date'

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
  const qc = useQueryClient()

  const { data: sessions = [], isLoading } = useQuery(accountSessionsQuery(accountId, open))

  const revokeMutation = useMutation({
    mutationFn: (sessionId: string) => api.revokeSession(sessionId),
    onSettled: () => qc.invalidateQueries({ queryKey: ['sessions', accountId] }),
  })

  const revokeAllMutation = useMutation({
    mutationFn: () => api.revokeAllSessions(accountId),
    onSettled: () => qc.invalidateQueries({ queryKey: ['sessions', accountId] }),
  })

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

function CreateAccountModal({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')
  const [role, setRole] = useState('admin')
  const [department, setDepartment] = useState('')
  const [position, setPosition] = useState('')
  const [created, setCreated] = useState<CreateAccountResponse | null>(null)

  const mutation = useMutation({
    mutationFn: () =>
      api.createAccount({ username, password, name, email: email || undefined, role, department: department || undefined, position: position || undefined }),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['accounts'] })
      setCreated(data)
    },
  })

  function handleClose() {
    setUsername('')
    setPassword('')
    setName('')
    setEmail('')
    setRole('admin')
    setDepartment('')
    setPosition('')
    setCreated(null)
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
              <Label>{t('accounts.username')}</Label>
              <Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" />
            </div>
            <div className="space-y-1.5">
              <Label>{t('accounts.fullName')}</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} />
            </div>
          </div>
          <div className="space-y-1.5">
            <Label>{t('accounts.password')}</Label>
            <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>{t('accounts.email')}</Label>
              <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>{t('accounts.role')}</Label>
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm"
                value={role}
                onChange={(e) => setRole(e.target.value)}
              >
                <option value="admin">admin</option>
                <option value="super">super</option>
              </select>
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>{t('accounts.department')}</Label>
              <Input value={department} onChange={(e) => setDepartment(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>{t('accounts.position')}</Label>
              <Input value={position} onChange={(e) => setPosition(e.target.value)} />
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
            disabled={mutation.isPending || !username || !password || !name}
          >
            {mutation.isPending ? t('accounts.creating') : t('common.create')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default function AccountsPage() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [showCreate, setShowCreate] = useState(false)
  const [resetToken, setResetToken] = useState<string | null>(null)
  const [sessionsAccountId, setSessionsAccountId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Account | null>(null)

  const { data: accounts = [], isLoading, isError } = useQuery(accountsQuery)

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

      <CreateAccountModal open={showCreate} onClose={() => setShowCreate(false)} />
      {sessionsAccountId && (
        <AccountSessionsModal
          accountId={sessionsAccountId}
          open={!!sessionsAccountId}
          onClose={() => setSessionsAccountId(null)}
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
              <TableHead>{t('accounts.username')}</TableHead>
              <TableHead>{t('accounts.name')}</TableHead>
              <TableHead>{t('accounts.role')}</TableHead>
              <TableHead>{t('accounts.department')}</TableHead>
              <TableHead>{t('accounts.status')}</TableHead>
              <TableHead>{t('accounts.lastLogin')}</TableHead>
              <TableHead>{t('accounts.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {accounts.map((a: Account) => (
                <TableRow key={a.id}>
                  <TableCell className="font-mono text-xs">{a.username}</TableCell>
                  <TableCell>{a.name}</TableCell>
                  <TableCell>
                    <Badge variant={a.role === 'super' ? 'default' : 'secondary'}>{a.role}</Badge>
                  </TableCell>
                  <TableCell className="text-muted-foreground text-sm">{a.department ?? '—'}</TableCell>
                  <TableCell>
                    <Switch
                      checked={a.is_active}
                      onCheckedChange={(v) => activeMutation.mutate({ id: a.id, is_active: v })}
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
                        title={t('accounts.sessions')}
                        onClick={() => setSessionsAccountId(a.id)}
                      >
                        <Shield className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        title={t('accounts.resetLink')}
                        onClick={() => resetMutation.mutate(a.id)}
                      >
                        <Link className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-destructive hover:text-destructive"
                        title={t('common.delete')}
                        onClick={() => setDeleteTarget(a)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))
            }
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
    </div>
  )
}
