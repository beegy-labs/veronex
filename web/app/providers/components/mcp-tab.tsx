'use client'

import { useState, useCallback, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mcpServersQuery } from '@/lib/queries/mcp'
import { api } from '@/lib/api'
import type { McpServer, McpServerStat, RegisterMcpServerRequest } from '@/lib/types'
import { Plus, Trash2, Plug, Bot, BarChart2 } from 'lucide-react'
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
            <Input id="mcp-name" value={name} onChange={(e) => handleNameChange(e.target.value)} placeholder="My MCP Server" />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="mcp-slug">{t('mcp.slug')} <span className="text-destructive">*</span></Label>
            <Input id="mcp-slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder="my_mcp_server" />
            <p className="text-xs text-muted-foreground">{t('mcp.slugHint')}</p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="mcp-url">{t('mcp.url')} <span className="text-destructive">*</span></Label>
            <Input id="mcp-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://mcp.example.com" />
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

const NONE_VALUE = '__none__'

function OrchestratorModelSelector() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [saved, setSaved] = useState(false)

  const { data: lab } = useQuery({
    queryKey: ['lab-settings'],
    queryFn: () => api.labSettings(),
  })

  const { data: syncSettings } = useQuery({
    queryKey: ['capacity-settings'],
    queryFn: () => api.syncSettings(),
  })

  const ollamaModels: string[] = syncSettings?.available_models?.ollama ?? []

  const mutation = useMutation({
    mutationFn: (model: string | null) =>
      api.patchLabSettings({ mcp_orchestrator_model: model }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['lab-settings'] })
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
    },
  })

  const current = lab?.mcp_orchestrator_model ?? null

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-medium flex items-center gap-2">
          <Bot className="h-4 w-4 text-muted-foreground" />
          {t('mcp.orchestratorModel')}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <p className="text-xs text-muted-foreground">{t('mcp.orchestratorModelDesc')}</p>
        <div className="flex items-center gap-2">
          <Select
            value={current ?? NONE_VALUE}
            onValueChange={(val) => mutation.mutate(val === NONE_VALUE ? null : val)}
            disabled={mutation.isPending}
          >
            <SelectTrigger className="w-72">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NONE_VALUE}>{t('mcp.orchestratorModelNone')}</SelectItem>
              {ollamaModels.map((m) => (
                <SelectItem key={m} value={m}>{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          {saved && (
            <span className="text-xs text-status-success-fg">{t('mcp.orchestratorModelSaved')}</span>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

const HOURS_OPTIONS = [
  { value: 1,   label: '1h' },
  { value: 6,   label: '6h' },
  { value: 24,  label: '24h' },
  { value: 168, label: '7d' },
  { value: 720, label: '30d' },
]

function fmt_pct(v: number) { return `${(v * 100).toFixed(1)}%` }
function fmt_ms(v: number) { return v < 1 ? '<1 ms' : `${Math.round(v)} ms` }

function McpStatsCard() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data: stats, isLoading, error } = useQuery({
    queryKey: ['mcp-stats', hours],
    queryFn: () => api.mcpStats(hours),
    staleTime: 30_000,
  })

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
                  <TableCell className="text-right tabular-nums">{s.total_calls.toLocaleString()}</TableCell>
                  <TableCell className="text-right tabular-nums">
                    <Badge
                      variant="outline"
                      className={s.success_rate >= 0.95
                        ? 'bg-status-success/15 text-status-success-fg border-status-success/30'
                        : s.success_rate >= 0.8
                          ? 'bg-status-warning/15 text-status-warning-fg border-status-warning/30'
                          : 'bg-status-error/15 text-status-error-fg border-status-error/30'}
                    >
                      {fmt_pct(s.success_rate)}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                    {s.cache_hit_count > 0 ? fmt_pct(s.cache_hit_count / s.total_calls) : '—'}
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                    {fmt_ms(s.avg_latency_ms)}
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

export function McpTab() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [showRegister, setShowRegister] = useState(false)
  const { hideSection } = useNav404()

  const { data: servers, isLoading, error } = useQuery(mcpServersQuery())

  // If the MCP API endpoint doesn't exist (404), hide the MCP nav item
  useEffect(() => {
    if ((error as { status?: number } | null)?.status === 404) {
      hideSection('mcp')
    }
  }, [error, hideSection])

  const toggleMutation = useMutation({
    mutationFn: ({ id, is_enabled }: { id: string; is_enabled: boolean }) =>
      api.patchMcpServer(id, { is_enabled }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteMcpServer(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }),
  })

  const handleDelete = useCallback((server: McpServer) => {
    if (confirm(t('mcp.deleteConfirm', { name: server.name }))) {
      deleteMutation.mutate(server.id)
    }
  }, [t, deleteMutation])

  return (
    <div className="space-y-4">
      <OrchestratorModelSelector />

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
              <TableHead className="whitespace-nowrap">Status</TableHead>
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
                  <Switch
                    checked={s.is_enabled}
                    disabled={toggleMutation.isPending}
                    onCheckedChange={(checked) => toggleMutation.mutate({ id: s.id, is_enabled: checked })}
                  />
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

      {showRegister && <RegisterMcpModal onClose={() => setShowRegister(false)} />}
    </div>
  )
}
