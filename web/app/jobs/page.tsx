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
    <div className="vds-space-y-4">
      {/* Controls */}
      <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-3">
        {data ? (
          <StatusPill icon={<ListOrdered className="vds-h-3 vds-w-3 vds-flex-shrink-0" />} count={data.total} label={t('jobs.totalLabel')} />
        ) : (
          <p className="vds-text-sm vds-text-dim vds-animate-pulse">{t('common.loading')}</p>
        )}
        <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap">
          {/* Search */}
          <div className="vds-relative vds-flex vds-items-center">
            <Search className="vds-absolute vds-left-2.5 vds-h-3.5 vds-w-3.5 vds-text-dim vds-pointer-events-none" />
            <Input
              className="vds-pl-8 vds-pr-8 vds-w-36 vds-sm:w-52 vds-h-9 vds-text-sm"
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
                className="vds-absolute vds-right-2.5 vds-text-dim vds-hover:text-primary"
                onClick={clearSearch}
              >
                <X className="vds-h-3.5 vds-w-3.5" />
              </button>
            )}
          </div>
          {/* Filter toggle button */}
          <Button
            variant={showFilters ? 'secondary' : 'outline'}
            size="sm"
            className="vds-h-9 vds-flex-shrink-0"
            onClick={() => setShowFilters((v) => !v)}
          >
            <SlidersHorizontal className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
            {activeFilterCount > 0 ? t('jobs.filtersActive', { count: activeFilterCount }) : t('jobs.filters')}
          </Button>
          <Button variant="ghost" size="icon" className="vds-h-9 vds-w-9 vds-flex-shrink-0" onClick={() => refetch()} disabled={isFetching}>
            <RefreshCw className={`vds-h-3.5 vds-w-3.5 ${isFetching ? 'vds-animate-spin' : ''}`} />
          </Button>
        </div>
      </div>

      {/* Filter panel */}
      {showFilters && (
        <div className="vds-flex vds-items-center vds-gap-2 vds-flex-wrap vds-p-3 vds-rounded-lg vds-border-1 vds-border-subtle vds-bg-muted/30">
          {!source && (
            <Select value={sourceFilter} onValueChange={(val) => { setSourceFilter(val); setPage(0) }}>
              <SelectTrigger className="vds-w-36 vds-h-9">
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
            <SelectTrigger className="vds-w-36 vds-h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('jobs.allProviders')}</SelectItem>
              <SelectItem value="ollama">{t('jobs.providerOllama')}</SelectItem>
              {geminiEnabled && <SelectItem value="gemini">{t('jobs.providerGemini')}</SelectItem>}
            </SelectContent>
          </Select>
          <Input
            className="vds-w-36 vds-h-9 vds-text-sm"
            placeholder={t('jobs.providerName')}
            value={serverNameFilter}
            onChange={(e) => { setServerNameFilter(e.target.value); setPage(0) }}
          />
          <Input
            className="vds-w-36 vds-h-9 vds-text-sm"
            placeholder={t('jobs.filterModel')}
            value={modelFilter}
            onChange={(e) => { setModelFilter(e.target.value); setPage(0) }}
          />
          <Select value={status} onValueChange={(val) => { setStatus(val); setPage(0) }}>
            <SelectTrigger className="vds-w-36 vds-h-9">
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
        <div className="vds-flex vds-items-center vds-gap-2 vds-text-sm vds-text-dim">
          <span>{t('jobs.searchingFor')}</span>
          <span className="vds-px-2 vds-py-0.5 vds-rounded vds-bg-primary/15 vds-text-primary vds-font-mono vds-text-xs">{query}</span>
          <button type="button" className="vds-underline vds-text-xs vds-hover:text-primary" onClick={clearSearch}>
            {t('jobs.clearSearch')}
          </button>
        </div>
      )}

      {isLoading && (
        <div className="vds-flex vds-h-48 vds-items-center vds-justify-center vds-text-dim">
          {t('jobs.loadingJobs')}
        </div>
      )}

      {error && (
        <Card className="vds-border-destructive/50 vds-bg-destructive/10">
          <CardContent className="vds-p-6">
            <p className="vds-font-600 vds-text-destructive">{t('jobs.failedJobs')}</p>
            <p className="vds-text-sm vds-mt-1 vds-text-destructive/80">
              {error instanceof Error ? error.message : t('common.unknownError')}
            </p>
          </CardContent>
        </Card>
      )}

      {data && <JobTable jobs={data.jobs} onRetry={onRetry} />}

      {/* Pagination */}
      {data && (
        <div className="vds-flex vds-items-center vds-justify-end vds-gap-4 vds-flex-wrap">
          <StatusPill label={data.total === 0 ? t('jobs.noJobs') : `${fmtNumber(firstItem)}–${fmtNumber(lastItem)} / ${fmtNumber(data.total)}`} />
          {totalPages > 1 && (
            <div className="vds-flex vds-items-center vds-gap-1">
              <Button variant="outline" size="icon" className="vds-h-8 vds-w-8"
                aria-label={t('common.prevPage')}
                onClick={() => goTo(page - 1)} disabled={page === 0}>
                <ChevronLeft className="vds-h-4 vds-w-4" />
              </Button>
              {buildPageSlots(page, totalPages).map((slot, i) =>
                slot === '…' ? (
                  <span key={`e-${i}`} className="vds-px-1.5 vds-text-dim vds-text-sm vds-select-none">…</span>
                ) : (
                  <Button key={slot} variant={slot === page ? 'default' : 'outline'}
                    size="icon" className="vds-h-8 vds-w-8 vds-text-xs" onClick={() => goTo(slot)}>
                    {slot + 1}
                  </Button>
                )
              )}
              <Button variant="outline" size="icon" className="vds-h-8 vds-w-8"
                aria-label={t('common.nextPage')}
                onClick={() => goTo(page + 1)} disabled={page >= totalPages - 1}>
                <ChevronRight className="vds-h-4 vds-w-4" />
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
  usePageGuard('dashboard_view')
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
      <div className="vds-space-y-6 vds-pb-20">
        <div>
          <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('jobs.title')}</h1>
          <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('jobs.description')}</p>
        </div>

        <Tabs value={activeTab} onValueChange={handleTabChange}>
          <TabsList>
            <TabsTrigger value="tasks">{t('jobs.tasks')}</TabsTrigger>
            <TabsTrigger value="conversations">{t('jobs.conversations')}</TabsTrigger>
            <TabsTrigger value="flow">{t('jobs.networkFlow')}</TabsTrigger>
          </TabsList>
          <TabsContent value="tasks" className="vds-mt-6">
            <JobsSection onRetry={handleRetry} />
          </TabsContent>
          <TabsContent value="conversations" className="vds-mt-6">
            <ConversationList onContinue={handleContinueConversation} />
          </TabsContent>
          <TabsContent value="flow" className="vds-mt-6">
            <NetworkFlowTab providers={providers ?? []} />
          </TabsContent>
        </Tabs>
      </div>

      {/* ── Floating bottom panel (Gmail-style) ─────────────────────────────── */}
      <div className="vds-fixed vds-bottom-0 vds-right-0 vds-sm:right-6 vds-z-popover vds-w-full vds-sm:w-[560px] vds-shadow-5 vds-rounded-t-xl vds-overflow-hidden vds-border-1 vds-border-subtle vds-bg-card">
        {/* Panel header — always visible */}
        <button
          type="button"
          className="vds-w-full vds-flex vds-items-center vds-gap-2 vds-px-4 vds-py-2.5 vds-bg-muted/80 vds-hover:bg-hover vds-transition-colors"
          onClick={() => setPanelOpen((v) => !v)}
        >
          <MessageSquare className="vds-h-4 vds-w-4 vds-text-dim vds-flex-shrink-0" />
          <span className="vds-text-sm vds-font-500 vds-flex-1 vds-text-left">{t('jobs.testPanel')}</span>
          {panelOpen ? <ChevronDown className="vds-h-4 vds-w-4 vds-text-dim" /> : <ChevronUp className="vds-h-4 vds-w-4 vds-text-dim" />}
        </button>

        {/* Panel body — slides open */}
        {panelOpen && (
          <div className="vds-max-h-[80vh] vds-overflow-y-auto">
            <div className="vds-p-4">
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
