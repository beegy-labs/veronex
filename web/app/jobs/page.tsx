'use client'

import { useState, useCallback, useMemo, useRef, useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { dashboardJobsQuery, providersQuery } from '@/lib/queries'
import { DASHBOARD_JOBS_QUERY_KEY } from '@/lib/queries/dashboard'
import { CONVERSATIONS_QUERY_KEY } from '@/lib/queries/conversations'
import { ConversationList } from './components/conversation-list'
import type { RetryParams, ConversationDetail } from '@/lib/types'
import JobTable from './components/job-table'
import dynamic from 'next/dynamic'
const ApiTestPanel = dynamic(() => import('./components/api-test-panel').then(m => ({ default: m.ApiTestPanel })), { ssr: false })
import { NetworkFlowTab } from '@/components/network-flow-tab'
import { ChevronLeft, ChevronRight, Search, X, ListOrdered, SlidersHorizontal, ChevronDown, ChevronUp, MessageSquare, RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { StatusPill } from '@/components/status-pill'
import { useLabSettings } from '@/components/lab-settings-provider'
import { fmtNumber } from '@/lib/date'
import { api } from '@/lib/api'

const PAGE_SIZE = 50

function buildPageSlots(current: number, total: number): (number | '…')[] {
  if (total <= 7) return Array.from({ length: total }, (_, i) => i)
  const first = 0
  const last = total - 1
  if (current <= 3) return [0, 1, 2, 3, 4, '…', last]
  if (current >= total - 4) return [first, '…', last - 4, last - 3, last - 2, last - 1, last]
  return [first, '…', current - 1, current, current + 1, '…', last]
}

// ── Reusable jobs section ──────────────────────────────────────────────────────

interface JobsSectionProps {
  source?: 'api' | 'test' | 'analyzer'
  onRetry?: (params: RetryParams) => void
}

function JobsSection({ source, onRetry }: JobsSectionProps) {
  const { t } = useTranslation()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false
  const [page, setPage] = useState(0)
  const [status, setStatus] = useState('all')
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [modelFilter, setModelFilter] = useState('')
  const [providerTypeFilter, setProviderTypeFilter] = useState('all')
  const [serverNameFilter, setServerNameFilter] = useState('')
  const [sourceFilter, setSourceFilter] = useState('all')
  const [showFilters, setShowFilters] = useState(false)

  const STATUS_OPTIONS = useMemo(() => [
    { value: 'all',       label: t('jobs.allStatuses') },
    { value: 'pending',   label: t('jobs.statuses.pending') },
    { value: 'running',   label: t('jobs.statuses.running') },
    { value: 'completed', label: t('jobs.statuses.completed') },
    { value: 'failed',    label: t('jobs.statuses.failed') },
    { value: 'cancelled', label: t('jobs.statuses.cancelled') },
  ], [t])

  const offset = page * PAGE_SIZE

  const resolvedSource = source ?? (sourceFilter !== 'all' ? sourceFilter as 'api' | 'test' | 'analyzer' : undefined)

  const { data, isLoading, isFetching, error, refetch } = useQuery(
    dashboardJobsQuery({ source: resolvedSource, page, status, query, pageSize: PAGE_SIZE, model: modelFilter || undefined, provider: serverNameFilter || undefined, providerType: providerTypeFilter !== 'all' ? providerTypeFilter : undefined }),
  )

  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0
  const firstItem = data && data.total > 0 ? offset + 1 : 0
  const lastItem = data ? Math.min(offset + PAGE_SIZE, data.total) : 0

  const commitSearch = useCallback(() => { setQuery(search); setPage(0) }, [search])
  const clearSearch = useCallback(() => { setSearch(''); setQuery(''); setPage(0) }, [])
  const goTo = (p: number) => setPage(Math.max(0, Math.min(totalPages - 1, p)))
  const activeFilterCount = (modelFilter ? 1 : 0) + (providerTypeFilter !== 'all' ? 1 : 0) + (serverNameFilter ? 1 : 0) + (status !== 'all' ? 1 : 0) + (sourceFilter !== 'all' ? 1 : 0)

  return (
    <div className="space-y-4">
      {/* Controls */}
      <div className="flex items-center justify-between flex-wrap gap-3">
        {data ? (
          <StatusPill icon={<ListOrdered className="h-3 w-3 shrink-0" />} count={data.total} label={t('jobs.totalLabel')} />
        ) : (
          <p className="text-sm text-muted-foreground animate-pulse">{t('common.loading')}</p>
        )}
        <div className="flex items-center gap-2 flex-wrap">
          {/* Search */}
          <div className="relative flex items-center">
            <Search className="absolute left-2.5 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
            <Input
              className="pl-8 pr-8 w-36 sm:w-52 h-9 text-sm"
              placeholder={t('jobs.searchPlaceholder')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitSearch()
                if (e.key === 'Escape') clearSearch()
              }}
            />
            {search && (
              <button
                type="button"
                aria-label={t('jobs.clearSearch')}
                className="absolute right-2.5 text-muted-foreground hover:text-foreground"
                onClick={clearSearch}
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
          {/* Filter toggle button */}
          <Button
            variant={showFilters ? 'secondary' : 'outline'}
            size="sm"
            className="h-9 shrink-0"
            onClick={() => setShowFilters((v) => !v)}
          >
            <SlidersHorizontal className="h-3.5 w-3.5 mr-1.5" />
            {activeFilterCount > 0 ? t('jobs.filtersActive', { count: activeFilterCount }) : t('jobs.filters')}
          </Button>
          <Button variant="ghost" size="icon" className="h-9 w-9 shrink-0" onClick={() => refetch()} disabled={isFetching}>
            <RefreshCw className={`h-3.5 w-3.5 ${isFetching ? 'animate-spin' : ''}`} />
          </Button>
        </div>
      </div>

      {/* Filter panel */}
      {showFilters && (
        <div className="flex items-center gap-2 flex-wrap p-3 rounded-lg border border-border bg-muted/30">
          {!source && (
            <Select value={sourceFilter} onValueChange={(val) => { setSourceFilter(val); setPage(0) }}>
              <SelectTrigger className="w-36 h-9">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('jobs.allSources')}</SelectItem>
                <SelectItem value="api">{t('jobs.sourceApi')}</SelectItem>
                <SelectItem value="test">{t('jobs.sourceTest')}</SelectItem>
                <SelectItem value="analyzer">{t('jobs.sourceAnalyzer')}</SelectItem>
              </SelectContent>
            </Select>
          )}
          <Select value={providerTypeFilter} onValueChange={(val) => { setProviderTypeFilter(val); setPage(0) }}>
            <SelectTrigger className="w-36 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('jobs.allProviders')}</SelectItem>
              <SelectItem value="ollama">{t('jobs.providerOllama')}</SelectItem>
              {geminiEnabled && <SelectItem value="gemini">{t('jobs.providerGemini')}</SelectItem>}
            </SelectContent>
          </Select>
          <Input
            className="w-36 h-9 text-sm"
            placeholder={t('jobs.providerName')}
            value={serverNameFilter}
            onChange={(e) => { setServerNameFilter(e.target.value); setPage(0) }}
          />
          <Input
            className="w-36 h-9 text-sm"
            placeholder={t('jobs.filterModel')}
            value={modelFilter}
            onChange={(e) => { setModelFilter(e.target.value); setPage(0) }}
          />
          <Select value={status} onValueChange={(val) => { setStatus(val); setPage(0) }}>
            <SelectTrigger className="w-36 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STATUS_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* Active search badge */}
      {query && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span>{t('jobs.searchingFor')}</span>
          <span className="px-2 py-0.5 rounded bg-primary/15 text-primary font-mono text-xs">{query}</span>
          <button type="button" className="underline text-xs hover:text-foreground" onClick={clearSearch}>
            {t('jobs.clearSearch')}
          </button>
        </div>
      )}

      {isLoading && (
        <div className="flex h-48 items-center justify-center text-muted-foreground">
          {t('jobs.loadingJobs')}
        </div>
      )}

      {error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-6">
            <p className="font-semibold text-destructive">{t('jobs.failedJobs')}</p>
            <p className="text-sm mt-1 text-destructive/80">
              {error instanceof Error ? error.message : t('common.unknownError')}
            </p>
          </CardContent>
        </Card>
      )}

      {data && <JobTable jobs={data.jobs} onRetry={onRetry} />}

      {/* Pagination */}
      {data && (
        <div className="flex items-center justify-end gap-4 flex-wrap">
          <StatusPill label={data.total === 0 ? t('jobs.noJobs') : `${fmtNumber(firstItem)}–${fmtNumber(lastItem)} / ${fmtNumber(data.total)}`} />
          {totalPages > 1 && (
            <div className="flex items-center gap-1">
              <Button variant="outline" size="icon" className="h-8 w-8"
                aria-label={t('common.prevPage')}
                onClick={() => goTo(page - 1)} disabled={page === 0}>
                <ChevronLeft className="h-4 w-4" />
              </Button>
              {buildPageSlots(page, totalPages).map((slot, i) =>
                slot === '…' ? (
                  <span key={`e-${i}`} className="px-1.5 text-muted-foreground text-sm select-none">…</span>
                ) : (
                  <Button key={slot} variant={slot === page ? 'default' : 'outline'}
                    size="icon" className="h-8 w-8 text-xs" onClick={() => goTo(slot)}>
                    {slot + 1}
                  </Button>
                )
              )}
              <Button variant="outline" size="icon" className="h-8 w-8"
                aria-label={t('common.nextPage')}
                onClick={() => goTo(page + 1)} disabled={page >= totalPages - 1}>
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function JobsPage() {
  usePageGuard('jobs')
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [panelOpen, setPanelOpen] = useState(false)
  const [retryParams, setRetryParams] = useState<RetryParams | null>(null)
  const [continueConversation, setContinueConversation] = useState<ConversationDetail | null>(null)

  const handleTurnComplete = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: DASHBOARD_JOBS_QUERY_KEY })
    queryClient.invalidateQueries({ queryKey: CONVERSATIONS_QUERY_KEY })
  }, [queryClient])

  const [activeTab, setActiveTab] = useState<'tasks' | 'conversations' | 'flow'>(() => {
    if (typeof window !== 'undefined') {
      const hash = window.location.hash.slice(1)
      if (hash === 'conversations' || hash === 'flow') return hash as 'conversations' | 'flow'
    }
    return 'tasks'
  })
  const handleTabChange = useCallback((v: string) => {
    const tab = v as 'tasks' | 'conversations' | 'flow'
    setActiveTab(tab)
    window.history.replaceState(null, '', `#${tab}`)
  }, [])

  const { data: providersData } = useQuery(providersQuery())
  const providers = providersData?.providers

  const handleRetry = useCallback((params: RetryParams) => {
    setRetryParams(params)
    setPanelOpen(true)
  }, [])

  const handleContinueConversation = useCallback((detail: ConversationDetail) => {
    setContinueConversation(detail)
    setPanelOpen(true)
  }, [])

  return (
    <>
      {/* ── Page content ────────────────────────────────────────────────────── */}
      <div className="space-y-6 pb-20">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('jobs.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('jobs.description')}</p>
        </div>

        <Tabs value={activeTab} onValueChange={handleTabChange}>
          <TabsList>
            <TabsTrigger value="tasks">{t('jobs.tasks')}</TabsTrigger>
            <TabsTrigger value="conversations">{t('jobs.conversations')}</TabsTrigger>
            <TabsTrigger value="flow">{t('jobs.networkFlow')}</TabsTrigger>
          </TabsList>
          <TabsContent value="tasks" className="mt-6">
            <JobsSection onRetry={handleRetry} />
          </TabsContent>
          <TabsContent value="conversations" className="mt-6">
            <ConversationList onContinue={handleContinueConversation} />
          </TabsContent>
          <TabsContent value="flow" className="mt-6">
            <NetworkFlowTab providers={providers ?? []} />
          </TabsContent>
        </Tabs>
      </div>

      {/* ── Floating bottom panel (Gmail-style) ─────────────────────────────── */}
      <div className="fixed bottom-0 right-0 sm:right-6 z-50 w-full sm:w-[560px] shadow-2xl rounded-t-xl overflow-hidden border border-border bg-card">
        {/* Panel header — always visible */}
        <button
          type="button"
          className="w-full flex items-center gap-2 px-4 py-2.5 bg-muted/80 hover:bg-muted transition-colors"
          onClick={() => setPanelOpen((v) => !v)}
        >
          <MessageSquare className="h-4 w-4 text-muted-foreground shrink-0" />
          <span className="text-sm font-medium flex-1 text-left">{t('jobs.testPanel')}</span>
          {panelOpen ? <ChevronDown className="h-4 w-4 text-muted-foreground" /> : <ChevronUp className="h-4 w-4 text-muted-foreground" />}
        </button>

        {/* Panel body — slides open */}
        {panelOpen && (
          <div className="max-h-[80vh] overflow-y-auto">
            <div className="p-4">
              <ApiTestPanel
                retryParams={retryParams}
                onRetryConsumed={() => setRetryParams(null)}
                onTurnComplete={handleTurnComplete}
                continueConversation={continueConversation}
                onContinueConsumed={() => setContinueConversation(null)}
              />
            </div>
          </div>
        )}
      </div>
    </>
  )
}
