'use client'

import { useState, useCallback, useEffect, useOptimistic } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mcpServersQuery, mcpStatsQuery, mcpSettingsQuery } from '@/lib/queries/mcp'
import { api } from '@/lib/api'
import { ApiHttpError } from '@/lib/types'
import type { McpServer, McpServerStat, McpToolSummary, McpSettings, RegisterMcpServerRequest, VerifyState } from '@/lib/types'
import { useVerifyUrl } from '@/hooks/use-verify-url'
import { Plus, Trash2, Plug, BarChart2, ChevronRight, Wrench, Pencil, CheckCircle2, XCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { fmtPct1, fmtMs, fmtCompact } from '@/lib/chart-theme'
import { useTranslation } from '@/i18n'
import { useNav404 } from '@/components/nav-404-context'

function VerifyUrlField({
  id, value, onChange, placeholder,
}: { id: string; value: string; onChange: (v: string) => void; placeholder?: string }) {
  const { t } = useTranslation()
  const { verifyState, verifyError, verify, handleUrlChange } = useVerifyUrl({
    verifyFn: api.verifyMcpServer,
    labels: {
      duplicate: t('mcp.verifyDuplicate'),
      network: t('mcp.verifyNetwork'),
      unreachable: t('mcp.verifyUnreachable'),
      fallback: t('mcp.verifyFailed'),
    },
  })

  function handleChange(v: string) { onChange(v); handleUrlChange() }

  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{t('mcp.url')} <span className="text-destructive">*</span></Label>
      <div className="flex gap-2">
        <Input
          id={id} type="url" value={value} placeholder={placeholder}
          onChange={(e) => handleChange(e.target.value)}
          className={verifyState === 'ok' ? 'border-status-success' : verifyState === 'error' ? 'border-destructive' : ''}
        />
        <Button type="button" variant="outline" size="sm" className="shrink-0"
          disabled={!value.trim() || verifyState === 'checking'}
          onClick={() => verify(value.trim())}>
          {verifyState === 'checking' ? t('mcp.verifying')
            : verifyState === 'ok' ? <><CheckCircle2 className="h-3.5 w-3.5 mr-1 text-status-success-fg" />{t('mcp.connected')}</>
            : t('mcp.verifyConnection')}
        </Button>
      </div>
      {verifyState === 'error' && (
        <p className="text-xs text-destructive flex items-center gap-1"><XCircle className="h-3 w-3" />{verifyError}</p>
      )}
    </div>
  )
}

function RegisterMcpModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [slug, setSlug] = useState('')
  const [url, setUrl] = useState('')
  const [timeout, setTimeout] = useState('30')
  const queryClient = useQueryClient()

  function handleNameChange(val: string) {
    setName(val)
    setSlug(val.toLowerCase().replace(/[^a-z0-9]/g, '_').replace(/_+/g, '_').replace(/^_|_$/g, ''))
  }

  const mutation = useMutation({
    mutationFn: () => {
      const body: RegisterMcpServerRequest = {
        name: name.trim(),
        slug: slug.trim(),
        url: url.trim(),
        timeout_secs: parseInt(timeout, 10) || 30,
      }
      return api.registerMcpServer(body)
    },
    onSettled: () => { queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }); onClose() },
  })

  const canSubmit = !!name.trim() && !!slug.trim() && !!url.trim() && !mutation.isPending

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('mcp.register')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="mcp-name">{t('mcp.name')} <span className="text-destructive">*</span></Label>
            <Input id="mcp-name" value={name} onChange={(e) => handleNameChange(e.target.value)} placeholder={t('mcp.namePlaceholder')} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="mcp-slug">{t('mcp.slug')} <span className="text-destructive">*</span></Label>
            <Input id="mcp-slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder={t('mcp.slugPlaceholder')} />
            <p className="text-xs text-muted-foreground">{t('mcp.slugHint')}</p>
          </div>

          <VerifyUrlField id="mcp-url" value={url} onChange={setUrl} placeholder={t('mcp.urlPlaceholder')} />

          <div className="space-y-1.5">
            <Label htmlFor="mcp-timeout">{t('mcp.timeout')}</Label>
            <Input id="mcp-timeout" type="number" min={1} max={300} value={timeout} onChange={(e) => setTimeout(e.target.value)} />
          </div>
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3 flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!canSubmit}>
            {mutation.isPending ? `${t('mcp.register')}…` : t('mcp.register')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}


function EditMcpModal({ server, onClose }: { server: McpServer; onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(server.name)
  const [slug, setSlug] = useState(server.slug)
  const [url, setUrl] = useState(server.url)
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => api.patchMcpServer(server.id, {
      name: name.trim(),
      slug: slug.trim(),
      url: url.trim(),
    }),
    onSettled: () => { queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }); onClose() },
  })

  const canSubmit = !!name.trim() && !!slug.trim() && !!url.trim() && !mutation.isPending

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('mcp.editTitle')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="edit-mcp-name">{t('mcp.name')} <span className="text-destructive">*</span></Label>
            <Input id="edit-mcp-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="edit-mcp-slug">{t('mcp.slug')} <span className="text-destructive">*</span></Label>
            <Input id="edit-mcp-slug" value={slug} onChange={(e) => setSlug(e.target.value)} />
            <p className="text-xs text-muted-foreground">{t('mcp.slugHint')}</p>
          </div>

          <VerifyUrlField id="edit-mcp-url" value={url} onChange={setUrl} />
        </div>

        {mutation.error && (
          <p className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="gap-3 flex-wrap">
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={() => mutation.mutate()} disabled={!canSubmit}>
            {mutation.isPending ? `${t('common.save')}…` : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}


function McpToolsDialog({ server, onClose }: { server: McpServer; onClose: () => void }) {
  const { t } = useTranslation()
  const tools = server.tools ?? []
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Wrench className="h-4 w-4 text-muted-foreground" />
            {server.name} — {t('mcp.toolList')}
          </DialogTitle>
        </DialogHeader>
        {tools.length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">{t('mcp.noTools')}</p>
        ) : (
          <div className="space-y-1 max-h-80 overflow-y-auto pr-1">
            {tools.map((tool) => (
              <div key={tool.namespaced_name} className="rounded-lg border px-3 py-2">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-xs bg-surface-code px-1.5 py-0.5 rounded text-text-dim">{tool.namespaced_name}</span>
                </div>
                {tool.description && (
                  <p className="text-xs text-muted-foreground mt-1 leading-relaxed">{tool.description}</p>
                )}
              </div>
            ))}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

const HOURS_OPTIONS = [
  { value: 1,   label: '1h' },
  { value: 6,   label: '6h' },
  { value: 24,  label: '24h' },
  { value: 168, label: '7d' },
  { value: 720, label: '30d' },
]

function McpStatsCard() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data: stats, isLoading, error } = useQuery(mcpStatsQuery(hours))

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between gap-2 flex-wrap">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <BarChart2 className="h-4 w-4 text-muted-foreground" />
            {t('mcp.stats')}
          </CardTitle>
          <Select value={String(hours)} onValueChange={(v) => setHours(Number(v))}>
            <SelectTrigger className="h-7 w-20 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {HOURS_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={String(o.value)}>{o.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <p className="text-xs text-muted-foreground">{t('mcp.statsDesc')}</p>
      </CardHeader>
      <CardContent>
        {isLoading && <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>}
        {error && <p className="text-sm text-destructive">{t('mcp.statsLoadError')}</p>}
        {stats && stats.length === 0 && (
          <p className="text-sm text-muted-foreground">{t('mcp.statsNoData')}</p>
        )}
        {stats && stats.length > 0 && (() => {
          // Group rows by server_slug preserving server order
          const groups: { slug: string; name: string; rows: McpServerStat[] }[] = []
          const seen = new Map<string, number>()
          for (const s of stats) {
            const idx = seen.get(s.server_slug)
            if (idx === undefined) {
              seen.set(s.server_slug, groups.length)
              groups.push({ slug: s.server_slug, name: s.server_name, rows: [s] })
            } else {
              groups[idx].rows.push(s)
            }
          }
          return (
            <DataTable minWidth="600px">
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead className="whitespace-nowrap">{t('mcp.name')}</TableHead>
                  <TableHead className="whitespace-nowrap">{t('mcp.statsToolName')}</TableHead>
                  <TableHead className="whitespace-nowrap text-right">{t('mcp.statsTotalCalls')}</TableHead>
                  <TableHead className="whitespace-nowrap text-right">{t('mcp.statsSuccessRate')}</TableHead>
                  <TableHead className="whitespace-nowrap text-right">{t('mcp.statsCacheHit')}</TableHead>
                  <TableHead className="whitespace-nowrap text-right">{t('mcp.statsAvgLatency')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groups.map((g) => g.rows.map((s, i) => (
                  <TableRow key={`${s.server_slug}:${s.tool_name}`}>
                    <TableCell>
                      {i === 0 ? (
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{g.name}</span>
                          <span className="font-mono text-xs text-muted-foreground bg-surface-code px-1.5 py-0.5 rounded">{g.slug}</span>
                        </div>
                      ) : (
                        <span className="invisible select-none">{g.slug}</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1.5">
                        {i > 0 && <ChevronRight className="h-3 w-3 text-muted-foreground/50 shrink-0" />}
                        <span className="font-mono text-xs text-text-dim bg-surface-code px-1.5 py-0.5 rounded">{s.tool_name}</span>
                      </div>
                    </TableCell>
                    <TableCell className="text-right tabular-nums">{fmtCompact(s.total_calls)}</TableCell>
                    <TableCell className="text-right tabular-nums">
                      <Badge
                        variant="outline"
                        className={s.success_rate >= 0.95
                          ? 'bg-status-success/15 text-status-success-fg border-status-success/30'
                          : s.success_rate >= 0.8
                            ? 'bg-status-warning/15 text-status-warning-fg border-status-warning/30'
                            : 'bg-status-error/15 text-status-error-fg border-status-error/30'}
                      >
                        {fmtPct1(s.success_rate * 100)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                      {s.cache_hit_count > 0 ? fmtPct1((s.cache_hit_count / s.total_calls) * 100) : '—'}
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                      {fmtMs(s.avg_latency_ms)}
                    </TableCell>
                  </TableRow>
                )))}
              </TableBody>
            </DataTable>
          )
        })()}
      </CardContent>
    </Card>
  )
}

function McpToggleSwitch({ serverId, isEnabled }: { serverId: string; isEnabled: boolean }) {
  const queryClient = useQueryClient()
  const [optimisticEnabled, setOptimistic] = useOptimistic(isEnabled, (_, v: boolean) => v)
  const mutation = useMutation({
    mutationFn: (is_enabled: boolean) => api.patchMcpServer(serverId, { is_enabled }),
    onError: () => setOptimistic(isEnabled),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }),
  })
  return (
    <Switch
      checked={optimisticEnabled}
      onCheckedChange={(checked) => { setOptimistic(checked); mutation.mutate(checked) }}
    />
  )
}

function McpSettingsPanel() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState<Partial<McpSettings>>({})

  const { data, isLoading, error } = useQuery(mcpSettingsQuery())

  const mutation = useMutation({
    mutationFn: (body: Partial<McpSettings>) => api.patchMcpSettings(body),
    onSuccess: () => { setEditing(false); setDraft({}) },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['mcp-settings'] }),
  })

  function startEdit() {
    if (data) setDraft({ ...data })
    setEditing(true)
  }

  function cancelEdit() {
    setEditing(false)
    setDraft({})
  }

  const current = editing ? draft : data

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between gap-2">
          <CardTitle className="text-sm font-medium">{t('mcp.settings')}</CardTitle>
          {!editing && (
            <Button variant="outline" size="sm" className="h-7 text-xs" onClick={startEdit} disabled={isLoading || !!error}>
              {t('common.edit')}
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {isLoading && <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>}
        {error && <p className="text-sm text-destructive">{t('mcp.settingsLoadError')}</p>}
        {current && (
          <>
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              {([
                { key: 'routing_cache_ttl_secs', label: t('mcp.routingCacheTtl') },
                { key: 'tool_schema_refresh_secs', label: t('mcp.toolSchemaRefresh') },
                { key: 'max_tools_per_request', label: t('mcp.maxToolsPerRequest') },
                { key: 'max_routing_cache_entries', label: t('mcp.maxRoutingCacheEntries') },
              ] as { key: keyof McpSettings; label: string }[]).map(({ key, label }) => (
                <div key={key} className="flex items-center justify-between gap-2 rounded-lg border px-3 py-2">
                  <Label className="text-xs text-muted-foreground shrink-0">{label}</Label>
                  {editing ? (
                    <Input
                      type="number"
                      className="h-7 w-28 text-xs text-right"
                      value={draft[key] as number ?? ''}
                      onChange={(e) => setDraft(prev => ({ ...prev, [key]: parseInt(e.target.value, 10) || 0 }))}
                    />
                  ) : (
                    <span className="text-sm tabular-nums font-mono">{current[key] as number}</span>
                  )}
                </div>
              ))}
            </div>

            <div className="flex items-center justify-between gap-2 rounded-lg border px-3 py-2">
              <Label className="text-xs text-muted-foreground shrink-0">{t('mcp.embeddingModel')}</Label>
              {editing ? (
                <Input
                  className="h-7 w-52 text-xs text-right font-mono"
                  value={draft.embedding_model ?? ''}
                  onChange={(e) => setDraft(prev => ({ ...prev, embedding_model: e.target.value }))}
                />
              ) : (
                <span className="text-sm font-mono text-muted-foreground">{current.embedding_model}</span>
              )}
            </div>

            {editing && (
              <div className="flex items-center gap-2 pt-1">
                <Button
                  size="sm"
                  disabled={mutation.isPending}
                  onClick={() => mutation.mutate(draft)}
                >
                  {mutation.isPending ? `${t('common.save')}…` : t('common.save')}
                </Button>
                <Button variant="outline" size="sm" onClick={cancelEdit}>{t('common.cancel')}</Button>
                {mutation.error && (
                  <span className="text-xs text-destructive">
                    {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
                  </span>
                )}
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}

export function McpTab() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [showRegister, setShowRegister] = useState(false)
  const [editTarget, setEditTarget] = useState<McpServer | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<McpServer | null>(null)
  const [toolsTarget, setToolsTarget] = useState<McpServer | null>(null)
  const { hideSection } = useNav404()

  const { data: servers, isLoading, error } = useQuery(mcpServersQuery())

  // If the MCP API endpoint doesn't exist (404), hide the MCP nav item
  useEffect(() => {
    if (error instanceof ApiHttpError && error.status === 404) {
      hideSection('mcp')
    }
  }, [error, hideSection])

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteMcpServer(id),
    onSuccess: () => setDeleteTarget(null),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }),
  })

  const handleDelete = useCallback((server: McpServer) => {
    setDeleteTarget(server)
  }, [])

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-end">
        <Button onClick={() => setShowRegister(true)}>
          <Plus className="h-4 w-4 mr-2" />{t('mcp.register')}
        </Button>
      </div>

      {isLoading && (
        <div className="flex h-24 items-center justify-center text-muted-foreground text-sm animate-pulse">
          {t('common.loading')}
        </div>
      )}

      {!isLoading && (!servers || servers.length === 0) && (
        <Card className="border-dashed">
          <CardContent className="p-8 text-center text-muted-foreground">
            <Plug className="h-8 w-8 mx-auto mb-3 opacity-25" />
            <p className="font-medium">{t('mcp.title')}</p>
            <p className="text-sm mt-1">{t('mcp.description')}</p>
          </CardContent>
        </Card>
      )}

      {servers && servers.length > 0 && (
        <DataTable minWidth="700px">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="whitespace-nowrap">{t('mcp.name')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('mcp.slug')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('mcp.url')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('mcp.status')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('mcp.tools')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('mcp.enabled')}</TableHead>
              <TableHead className="text-right whitespace-nowrap">{t('keys.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {servers.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="font-semibold text-text-bright">{s.name}</TableCell>
                <TableCell>
                  <span className="font-mono text-xs text-text-dim bg-surface-code px-2 py-1 rounded">{s.slug}</span>
                </TableCell>
                <TableCell>
                  <span className="font-mono text-xs text-text-dim truncate max-w-48 block">{s.url}</span>
                </TableCell>
                <TableCell>
                  <span className={`flex items-center gap-1.5 text-xs ${s.online ? 'text-status-success-fg' : 'text-muted-foreground'}`}>
                    <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${s.online ? 'bg-status-success' : 'bg-muted-foreground'}`} />
                    {s.online ? t('mcp.online') : t('mcp.offline')}
                  </span>
                </TableCell>
                <TableCell>
                  <button
                    type="button"
                    onClick={() => setToolsTarget(s)}
                    className="text-sm text-muted-foreground hover:text-foreground hover:underline underline-offset-2 cursor-pointer transition-colors"
                  >
                    {s.tool_count} {t('mcp.tools')}
                  </button>
                </TableCell>
                <TableCell>
                  <McpToggleSwitch serverId={s.id} isEnabled={s.is_enabled} />
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-foreground"
                      aria-label={t('common.edit')}
                      onClick={() => setEditTarget(s)}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-status-error-fg hover:bg-status-error/10"
                      aria-label={t('common.delete')}
                      onClick={() => handleDelete(s)}
                      disabled={deleteMutation.isPending}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </DataTable>
      )}

      <McpStatsCard />

      <McpSettingsPanel />

      {showRegister && <RegisterMcpModal onClose={() => setShowRegister(false)} />}
      {editTarget && <EditMcpModal server={editTarget} onClose={() => setEditTarget(null)} />}
      {toolsTarget && <McpToolsDialog server={toolsTarget} onClose={() => setToolsTarget(null)} />}

      {deleteTarget && (
        <ConfirmDialog
          open
          title={t('mcp.deleteTitle')}
          description={t('mcp.deleteConfirm', { name: deleteTarget.name })}
          confirmLabel={deleteMutation.isPending ? t('common.deleting') : t('common.delete')}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isLoading={deleteMutation.isPending}
        />
      )}
    </div>
  )
}
