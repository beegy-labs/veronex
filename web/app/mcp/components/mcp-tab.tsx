'use client'

import { useState, useCallback, useEffect, useOptimistic } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mcpServersQuery, mcpStatsQuery, mcpSettingsQuery } from '@/lib/queries/mcp'
import { api } from '@/lib/api'
import { ApiHttpError } from '@/lib/types'
import type { McpServer, McpServerStat, McpSettings, RegisterMcpServerRequest } from '@/lib/types'
import { Plus, Trash2, Plug, BarChart2 } from 'lucide-react'
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

          <div className="space-y-1.5">
            <Label htmlFor="mcp-url">{t('mcp.url')} <span className="text-destructive">*</span></Label>
            <Input id="mcp-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder={t('mcp.urlPlaceholder')} />
          </div>

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
        {stats && stats.length > 0 && (
          <DataTable minWidth="540px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="whitespace-nowrap">{t('mcp.name')}</TableHead>
                <TableHead className="whitespace-nowrap text-right">{t('mcp.statsTotalCalls')}</TableHead>
                <TableHead className="whitespace-nowrap text-right">{t('mcp.statsSuccessRate')}</TableHead>
                <TableHead className="whitespace-nowrap text-right">{t('mcp.statsCacheHit')}</TableHead>
                <TableHead className="whitespace-nowrap text-right">{t('mcp.statsAvgLatency')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {stats.map((s: McpServerStat) => (
                <TableRow key={s.server_slug}>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{s.server_name}</span>
                      <span className="font-mono text-xs text-muted-foreground bg-surface-code px-1.5 py-0.5 rounded">{s.server_slug}</span>
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
              ))}
            </TableBody>
          </DataTable>
        )}
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
          <CardTitle className="text-sm font-medium">MCP Settings</CardTitle>
          {!editing && (
            <Button variant="outline" size="sm" className="h-7 text-xs" onClick={startEdit} disabled={isLoading || !!error}>
              Edit
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {isLoading && <p className="text-sm text-muted-foreground animate-pulse">Loading…</p>}
        {error && <p className="text-sm text-destructive">Failed to load MCP settings.</p>}
        {current && (
          <>
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              {([
                { key: 'routing_cache_ttl_secs', label: 'Routing Cache TTL (s)' },
                { key: 'tool_schema_refresh_secs', label: 'Tool Schema Refresh (s)' },
                { key: 'max_tools_per_request', label: 'Max Tools / Request' },
                { key: 'max_routing_cache_entries', label: 'Max Cache Entries' },
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
              <Label className="text-xs text-muted-foreground shrink-0">Embedding Model</Label>
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
  const [deleteTarget, setDeleteTarget] = useState<McpServer | null>(null)
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
                <TableCell className="text-muted-foreground text-sm">
                  {s.tool_count} {t('mcp.tools')}
                </TableCell>
                <TableCell>
                  <McpToggleSwitch serverId={s.id} isEnabled={s.is_enabled} />
                </TableCell>
                <TableCell className="text-right">
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
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </DataTable>
      )}

      <McpStatsCard />

      <McpSettingsPanel />

      {showRegister && <RegisterMcpModal onClose={() => setShowRegister(false)} />}

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
