'use client'

import { useState, useCallback } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mcpServersQuery } from '@/lib/queries/mcp'
import { api } from '@/lib/api'
import type { McpServer, RegisterMcpServerRequest } from '@/lib/types'
import { Plus, Trash2, Plug } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'

// ── Register modal ─────────────────────────────────────────────────────────────

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

// ── Page ───────────────────────────────────────────────────────────────────────

export default function McpPage() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [showRegister, setShowRegister] = useState(false)

  const { data: servers, isLoading } = useQuery(mcpServersQuery())

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
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('mcp.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('mcp.description')}</p>
      </div>

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

      {showRegister && <RegisterMcpModal onClose={() => setShowRegister(false)} />}
    </div>
  )
}
