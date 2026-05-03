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
    <div className="vds-space-y-1.5">
      <Label htmlFor={id}>{t('mcp.url')} <span className="vds-text-destructive">*</span></Label>
      <div className="vds-flex vds-gap-2">
        <Input
          id={id} type="url" value={value} placeholder={placeholder}
          onChange={(e) => handleChange(e.target.value)}
          className={verifyState === 'ok' ? 'vds-border-success' : verifyState === 'error' ? 'vds-border-destructive' : ''}
        />
        <Button type="button" variant="outline" size="sm" className="vds-flex-shrink-0"
          disabled={!value.trim() || verifyState === 'checking'}
          onClick={() => verify(value.trim())}>
          {verifyState === 'checking' ? t('mcp.verifying')
            : verifyState === 'ok' ? <><CheckCircle2 className="vds-h-3.5 vds-w-3.5 vds-mr-1 vds-text-success" />{t('mcp.connected')}</>
            : t('mcp.verifyConnection')}
        </Button>
      </div>
      {verifyState === 'error' && (
        <p className="vds-text-xs vds-text-destructive vds-flex vds-items-center vds-gap-1"><XCircle className="vds-h-3 vds-w-3" />{verifyError}</p>
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
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle>{t('mcp.register')}</DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="mcp-name">{t('mcp.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="mcp-name" value={name} onChange={(e) => handleNameChange(e.target.value)} placeholder={t('mcp.namePlaceholder')} />
          </div>

          <div className="vds-space-y-1.5">
            <Label htmlFor="mcp-slug">{t('mcp.slug')} <span className="vds-text-destructive">*</span></Label>
            <Input id="mcp-slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder={t('mcp.slugPlaceholder')} />
            <p className="vds-text-xs vds-text-dim">{t('mcp.slugHint')}</p>
          </div>

          <VerifyUrlField id="mcp-url" value={url} onChange={setUrl} placeholder={t('mcp.urlPlaceholder')} />

          <div className="vds-space-y-1.5">
            <Label htmlFor="mcp-timeout">{t('mcp.timeout')}</Label>
            <Input id="mcp-timeout" type="number" min={1} max={300} value={timeout} onChange={(e) => setTimeout(e.target.value)} />
          </div>
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
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
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle>{t('mcp.editTitle')}</DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4">
          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-mcp-name">{t('mcp.name')} <span className="vds-text-destructive">*</span></Label>
            <Input id="edit-mcp-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          <div className="vds-space-y-1.5">
            <Label htmlFor="edit-mcp-slug">{t('mcp.slug')} <span className="vds-text-destructive">*</span></Label>
            <Input id="edit-mcp-slug" value={slug} onChange={(e) => setSlug(e.target.value)} />
            <p className="vds-text-xs vds-text-dim">{t('mcp.slugHint')}</p>
          </div>

          <VerifyUrlField id="edit-mcp-url" value={url} onChange={setUrl} />
        </div>

        {mutation.error && (
          <p className="vds-text-sm vds-text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : t('common.error')}
          </p>
        )}

        <DialogFooter className="vds-gap-3 vds-flex-wrap">
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
      <DialogContent className="vds-max-w-md">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <Wrench className="vds-h-4 vds-w-4 vds-text-dim" />
            {server.name} — {t('mcp.toolList')}
          </DialogTitle>
        </DialogHeader>
        {tools.length === 0 ? (
          <p className="vds-text-sm vds-text-dim vds-py-2">{t('mcp.noTools')}</p>
        ) : (
          <div className="vds-space-y-1 vds-max-h-80 vds-overflow-y-auto vds-pr-1">
            {tools.map((tool) => (
              <div key={tool.namespaced_name} className="vds-rounded-lg vds-border-1 vds-px-3 vds-py-2">
                <div className="vds-flex vds-items-center vds-gap-2">
                  <span className="vds-font-mono vds-text-xs vds-bg-surface-code vds-px-1.5 vds-py-0.5 vds-rounded vds-text-dim">{tool.namespaced_name}</span>
                </div>
                {tool.description && (
                  <p className="vds-text-xs vds-text-dim vds-mt-1 vds-leading-relaxed">{tool.description}</p>
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
      <CardHeader className="vds-pb-3">
        <div className="vds-flex vds-items-center vds-justify-between vds-gap-2 vds-flex-wrap">
          <CardTitle className="vds-text-sm vds-font-500 vds-flex vds-items-center vds-gap-2">
            <BarChart2 className="vds-h-4 vds-w-4 vds-text-dim" />
            {t('mcp.stats')}
          </CardTitle>
          <Select value={String(hours)} onValueChange={(v) => setHours(Number(v))}>
            <SelectTrigger className="vds-h-7 vds-w-20 vds-text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {HOURS_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={String(o.value)}>{o.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <p className="vds-text-xs vds-text-dim">{t('mcp.statsDesc')}</p>
      </CardHeader>
      <CardContent>
        {isLoading && <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>}
        {error && <p className="vds-text-sm vds-text-destructive">{t('mcp.statsLoadError')}</p>}
        {stats && stats.length === 0 && (
          <p className="vds-text-sm vds-text-dim">{t('mcp.statsNoData')}</p>
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
                <TableRow className="vds-hover:bg-transparent">
                  <TableHead className="vds-whitespace-nowrap">{t('mcp.name')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap">{t('mcp.statsToolName')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap vds-text-right">{t('mcp.statsTotalCalls')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap vds-text-right">{t('mcp.statsSuccessRate')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap vds-text-right">{t('mcp.statsCacheHit')}</TableHead>
                  <TableHead className="vds-whitespace-nowrap vds-text-right">{t('mcp.statsAvgLatency')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groups.map((g) => g.rows.map((s, i) => (
                  <TableRow key={`${s.server_slug}:${s.tool_name}`}>
                    <TableCell>
                      {i === 0 ? (
                        <div className="vds-flex vds-items-center vds-gap-2">
                          <span className="vds-font-500">{g.name}</span>
                          <span className="vds-font-mono vds-text-xs vds-text-dim vds-bg-surface-code vds-px-1.5 vds-py-0.5 vds-rounded">{g.slug}</span>
                        </div>
                      ) : (
                        <span className="vds-invisible vds-select-none">{g.slug}</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="vds-flex vds-items-center vds-gap-1.5">
                        {i > 0 && <ChevronRight className="vds-h-3 vds-w-3 vds-text-dim/50 vds-flex-shrink-0" />}
                        <span className="vds-font-mono vds-text-xs vds-text-dim vds-bg-surface-code vds-px-1.5 vds-py-0.5 vds-rounded">{s.tool_name}</span>
                      </div>
                    </TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums">{fmtCompact(s.total_calls)}</TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums">
                      <Badge
                        variant="outline"
                        className={s.success_rate >= 0.95
                          ? 'vds-bg-success/15 vds-text-success vds-border-success/30'
                          : s.success_rate >= 0.8
                            ? 'vds-bg-warning/15 vds-text-warning vds-border-warning/30'
                            : 'vds-bg-error/15 vds-text-error vds-border-error/30'}
                      >
                        {fmtPct1(s.success_rate * 100)}
                      </Badge>
                    </TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">
                      {s.cache_hit_count > 0 ? fmtPct1((s.cache_hit_count / s.total_calls) * 100) : '—'}
                    </TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums vds-text-dim vds-text-sm">
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
      <CardHeader className="vds-pb-3">
        <div className="vds-flex vds-items-center vds-justify-between vds-gap-2">
          <CardTitle className="vds-text-sm vds-font-500">{t('mcp.settings')}</CardTitle>
          {!editing && (
            <Button variant="outline" size="sm" className="vds-h-7 vds-text-xs" onClick={startEdit} disabled={isLoading || !!error}>
              {t('common.edit')}
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="vds-space-y-3">
        {isLoading && <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>}
        {error && <p className="vds-text-sm vds-text-destructive">{t('mcp.settingsLoadError')}</p>}
        {current && (
          <>
            <div className="vds-grid vds-grid-cols-1 vds-gap-2.5 vds-sm:grid-cols-2">
              {([
                { key: 'routing_cache_ttl_secs', label: t('mcp.routingCacheTtl') },
                { key: 'tool_schema_refresh_secs', label: t('mcp.toolSchemaRefresh') },
                { key: 'max_tools_per_request', label: t('mcp.maxToolsPerRequest') },
                { key: 'max_routing_cache_entries', label: t('mcp.maxRoutingCacheEntries') },
              ] as { key: keyof McpSettings; label: string }[]).map(({ key, label }) => (
                <div key={key} className="vds-flex vds-items-center vds-justify-between vds-gap-2 vds-rounded-lg vds-border-1 vds-px-3 vds-py-2">
                  <Label className="vds-text-xs vds-text-dim vds-flex-shrink-0">{label}</Label>
                  {editing ? (
                    <Input
                      type="number"
                      className="vds-h-7 vds-w-28 vds-text-xs vds-text-right"
                      value={draft[key] as number ?? ''}
                      onChange={(e) => setDraft(prev => ({ ...prev, [key]: parseInt(e.target.value, 10) || 0 }))}
                    />
                  ) : (
                    <span className="vds-text-sm vds-tabular-nums vds-font-mono">{current[key] as number}</span>
                  )}
                </div>
              ))}
            </div>

            <div className="vds-flex vds-items-center vds-justify-between vds-gap-2 vds-rounded-lg vds-border-1 vds-px-3 vds-py-2">
              <Label className="vds-text-xs vds-text-dim vds-flex-shrink-0">{t('mcp.embeddingModel')}</Label>
              {editing ? (
                <Input
                  className="vds-h-7 vds-w-52 vds-text-xs vds-text-right vds-font-mono"
                  value={draft.embedding_model ?? ''}
                  onChange={(e) => setDraft(prev => ({ ...prev, embedding_model: e.target.value }))}
                />
              ) : (
                <span className="vds-text-sm vds-font-mono vds-text-dim">{current.embedding_model}</span>
              )}
            </div>

            {editing && (
              <div className="vds-flex vds-items-center vds-gap-2 vds-pt-1">
                <Button
                  size="sm"
                  disabled={mutation.isPending}
                  onClick={() => mutation.mutate(draft)}
                >
                  {mutation.isPending ? `${t('common.save')}…` : t('common.save')}
                </Button>
                <Button variant="outline" size="sm" onClick={cancelEdit}>{t('common.cancel')}</Button>
                {mutation.error && (
                  <span className="vds-text-xs vds-text-destructive">
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
 <div className="vds-space-y-4">
 <div className="vds-flex vds-items-center vds-justify-end">
 <Button onClick={() => setShowRegister(true)}>
 <Plus className="vds-h-4 vds-w-4 vds-mr-2" />{t('mcp.register')}
 </Button>
 </div>

 {isLoading && (
 <div className="vds-flex vds-h-24 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
 {t('common.loading')}
 </div>
 )}

 {!isLoading && (!servers || servers.length === 0) && (
 <Card className="vds-border-dashed">
 <CardContent className="vds-p-8 vds-text-center vds-text-dim">
 <Plug className="vds-h-8 vds-w-8 vds-mx-auto vds-mb-3 vds-opacity-25" />
 <p className="vds-font-500">{t('mcp.title')}</p>
 <p className="vds-text-sm vds-mt-1">{t('mcp.description')}</p>
 </CardContent>
 </Card>
 )}

 {servers && servers.length > 0 && (
 <DataTable minWidth="700px">
 <TableHeader>
 <TableRow className="vds-hover:bg-transparent">
 <TableHead className="vds-whitespace-nowrap">{t('mcp.name')}</TableHead>
 <TableHead className="vds-whitespace-nowrap">{t('mcp.slug')}</TableHead>
 <TableHead className="vds-whitespace-nowrap">{t('mcp.url')}</TableHead>
 <TableHead className="vds-whitespace-nowrap">{t('mcp.status')}</TableHead>
 <TableHead className="vds-whitespace-nowrap">{t('mcp.tools')}</TableHead>
 <TableHead className="vds-whitespace-nowrap">{t('mcp.enabled')}</TableHead>
 <TableHead className="vds-text-right vds-whitespace-nowrap">{t('keys.actions')}</TableHead>
 </TableRow>
 </TableHeader>
 <TableBody>
 {servers.map((s) => (
 <TableRow key={s.id}>
 <TableCell className="vds-font-600 vds-text-bright">{s.name}</TableCell>
 <TableCell>
 <span className="vds-font-mono vds-text-xs vds-text-dim vds-bg-surface-code vds-px-2 vds-py-1 vds-rounded">{s.slug}</span>
 </TableCell>
 <TableCell>
 <span className="vds-font-mono vds-text-xs vds-text-dim vds-truncate vds-max-w-48 vds-block">{s.url}</span>
 </TableCell>
 <TableCell>
 <span className={`vds-flex vds-items-center vds-gap-1.5 vds-text-xs ${s.online ?'vds-text-success' : 'vds-text-dim'}`}>
 <span className={`vds-h-1.5 vds-w-1.5 vds-rounded-full vds-flex-shrink-0 ${s.online ? 'vds-bg-success' : 'vds-bg-neutral'}`} />
                    {s.online ? t('mcp.online') : t('mcp.offline')}
 </span>
 </TableCell>
 <TableCell>
 <button
 type="button"
 onClick={() => setToolsTarget(s)}
 className="vds-text-sm vds-text-dim vds-hover:text-primary vds-hover:underline vds-underline-offset-2 vds-cursor-pointer vds-transition-colors"
 >
 {s.tool_count} {t('mcp.tools')}
 </button>
 </TableCell>
 <TableCell>
 <McpToggleSwitch serverId={s.id} isEnabled={s.is_enabled} />
 </TableCell>
 <TableCell className="vds-text-right">
 <div className="vds-flex vds-items-center vds-justify-end vds-gap-1">
 <Button
 variant="ghost"
 size="icon"
 className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-primary"
 aria-label={t('common.edit')}
 onClick={() => setEditTarget(s)}
 >
 <Pencil className="vds-h-4 vds-w-4" />
 </Button>
 <Button
 variant="ghost"
 size="icon"
 className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-error vds-hover:bg-error/10"
 aria-label={t('common.delete')}
 onClick={() => handleDelete(s)}
 disabled={deleteMutation.isPending}
 >
 <Trash2 className="vds-h-4 vds-w-4" />
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
