'use client'

import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { auditQuery } from '@/lib/queries'
import type { AuditEvent } from '@/lib/types'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable, DataTableEmpty } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime } from '@/lib/date'

const ACTION_COLORS: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
  create: 'default',
  update: 'secondary',
  delete: 'destructive',
  login: 'outline',
  logout: 'outline',
  reset_password: 'secondary',
}

export default function AuditPage() {
  usePageGuard('audit')
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [action, setAction] = useState<string>('all')
  const [resourceType, setResourceType] = useState<string>('all')
  const [page, setPage] = useState(0)
  const PAGE_SIZE = 50

  const { data: events = [], isLoading, isError, refetch } = useQuery(auditQuery(action, resourceType))

  const totalPages = Math.max(1, Math.ceil(events.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages - 1)
  const pageItems = useMemo(() => events.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE), [events, safePage])

  return (
    <div className="flex flex-col gap-6 p-6 max-w-6xl mx-auto">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">{t('audit.title')}</h1>
          <p className="text-sm text-muted-foreground mt-0.5">{t('audit.description')}</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          {t('common.refresh')}
        </Button>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3">
        <Select value={action} onValueChange={setAction}>
          <SelectTrigger className="w-44">
            <SelectValue placeholder={t('audit.filterAction')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('audit.allActions')}</SelectItem>
            <SelectItem value="create">create</SelectItem>
            <SelectItem value="update">update</SelectItem>
            <SelectItem value="delete">delete</SelectItem>
            <SelectItem value="login">login</SelectItem>
            <SelectItem value="logout">logout</SelectItem>
            <SelectItem value="reset_password">reset_password</SelectItem>
          </SelectContent>
        </Select>

        <Select value={resourceType} onValueChange={setResourceType}>
          <SelectTrigger className="w-48">
            <SelectValue placeholder={t('audit.filterResource')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('audit.allResources')}</SelectItem>
            <SelectItem value="account">account</SelectItem>
            <SelectItem value="api_key">api_key</SelectItem>
            <SelectItem value="ollama_provider">ollama_provider</SelectItem>
            <SelectItem value="gemini_provider">gemini_provider</SelectItem>
            <SelectItem value="gpu_server">gpu_server</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {isLoading ? (
        <p className="text-sm text-muted-foreground">{t('common.loading')}</p>
      ) : isError ? (
        <p className="text-sm text-destructive">{t('common.error')}</p>
      ) : events.length === 0 ? (
        <DataTableEmpty>{t('audit.noEvents')}</DataTableEmpty>
      ) : (
        <>
        <DataTable minWidth="800px">
          <TableHeader>
            <TableRow>
              <TableHead className="whitespace-nowrap">{t('audit.time')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('audit.account')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('audit.action')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('audit.resourceType')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('audit.resourceName')}</TableHead>
              <TableHead className="whitespace-nowrap">{t('audit.ip')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {pageItems.map((e: AuditEvent) => (
                <TableRow key={`${e.event_time}-${e.account_id}-${e.action}-${e.resource_id}`}>
                  <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                    {fmtDatetime(e.event_time, tz)}
                  </TableCell>
                  <TableCell className="font-mono text-xs">{e.account_name}</TableCell>
                  <TableCell>
                    <Badge variant={ACTION_COLORS[e.action] ?? 'outline'} className="text-xs whitespace-nowrap">
                      {e.action}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">{e.resource_type}</TableCell>
                  <TableCell className="text-sm">{e.resource_name || e.resource_id}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">{e.ip_address || '—'}</TableCell>
                </TableRow>
              ))
            }
          </TableBody>
        </DataTable>
        {totalPages > 1 && (
          <div className="flex items-center justify-end gap-2">
            <span className="text-xs text-muted-foreground tabular-nums">
              {safePage * PAGE_SIZE + 1}–{Math.min((safePage + 1) * PAGE_SIZE, events.length)} / {events.length}
            </span>
            <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage <= 0}
              onClick={() => setPage(p => p - 1)}>
              <ChevronLeft className="h-3.5 w-3.5" />
            </Button>
            <Button variant="outline" size="icon" className="h-7 w-7" disabled={safePage >= totalPages - 1}
              onClick={() => setPage(p => p + 1)}>
              <ChevronRight className="h-3.5 w-3.5" />
            </Button>
          </div>
        )}
        </>
      )}
    </div>
  )
}
