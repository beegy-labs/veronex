'use client'

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { GeminiRateLimitPolicy, GeminiStatusResult } from '@/lib/types'
import { geminiPoliciesQuery, geminiModelsQuery, geminiSyncConfigQuery } from '@/lib/queries'
import { RotateCcw, RefreshCw, ShieldCheck, Pencil } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import {
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtDateOnly, fmtDatetimeShort } from '@/lib/date'
import { GEMINI_QUERY_KEYS } from '@/lib/queries/providers'
import {
  PROVIDER_STATUS_DOT_ALT, PROVIDER_STATUS_TEXT, PROVIDER_STATUS_I18N,
} from '@/lib/constants'
import { EditPolicyModal, SetSyncKeyModal } from './modals'

// ── Gemini Status Sync Section ─────────────────────────────────────────────────

export function statusDotCls(s: string) { return PROVIDER_STATUS_DOT_ALT[s] ?? PROVIDER_STATUS_DOT_ALT.offline }
export function statusResultCls(s: string) { return PROVIDER_STATUS_TEXT[s] ?? PROVIDER_STATUS_TEXT.offline }
export function statusResultLabel(s: string, t: (k: string) => string) {
  const key = PROVIDER_STATUS_I18N[s] ?? PROVIDER_STATUS_I18N.offline
  return t(key)
}

export function GeminiStatusSyncSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiStatus(),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['providers'] })
    },
  })

  const results: GeminiStatusResult[] = syncMutation.data?.results ?? []
  const onlineCount = results.filter((r) => r.status === 'online').length

  return (
    <div className="space-y-3">
      <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
        <RefreshCw className="h-4 w-4 text-accent-gpu" />
        {t('providers.gemini.statusSyncSection')}
      </h2>

      <Card>
        <CardContent className="p-4 space-y-4">
          <p className="text-sm text-muted-foreground">{t('providers.gemini.statusSyncDesc')}</p>

          <div className="flex items-center gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="gap-1.5">
              <RefreshCw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('providers.gemini.syncingStatus') : t('providers.gemini.syncStatus')}
            </Button>
            {syncMutation.isSuccess && !syncMutation.isPending && (
              <span className="text-xs text-status-success-fg">
                ✓ {t('providers.gemini.statusSyncDone')} — {onlineCount}/{results.length} {t('common.online').toLowerCase()}
              </span>
            )}
          </div>

          {syncMutation.isSuccess && results.length === 0 && (
            <p className="text-xs text-muted-foreground italic">{t('providers.gemini.noStatusResults')}</p>
          )}

          {results.length > 0 && (
            <div className="divide-y divide-border rounded-md border border-border overflow-hidden">
              {results.map((r) => (
                <div key={r.id} className="flex items-center gap-3 px-3 py-2.5">
                  <span className={statusDotCls(r.status)} />
                  <span className="font-medium text-sm text-text-bright flex-1 truncate">{r.name}</span>
                  <span className={`text-xs font-medium ${statusResultCls(r.status)}`}>
                    {statusResultLabel(r.status, t)}
                  </span>
                  {r.error && (
                    <span className="text-xs text-status-error-fg truncate max-w-[160px]" title={r.error}>
                      {r.error}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// ── Gemini Sync Section ────────────────────────────────────────────────────────

export function GeminiSyncSection() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const queryClient = useQueryClient()
  const [showSetKey, setShowSetKey] = useState(false)
  const [editingPolicy, setEditingPolicy] = useState<GeminiRateLimitPolicy | null>(null)

  // SSOT: all Gemini data refresh in one place — used by sync button and refresh button
  function refreshGeminiData() {
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.models })
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.policies })
    // Also refresh per-provider model selections so ModelSelectionModal picks up new models
    queryClient.invalidateQueries({ queryKey: GEMINI_QUERY_KEYS.selectedModels })
  }

  const { data: syncConfig } = useQuery(geminiSyncConfigQuery)

  const { data: modelsData, isLoading: modelsLoading, isFetching: modelsFetching } = useQuery(geminiModelsQuery)

  const { data: policies, isLoading: policiesLoading, isFetching: policiesFetching } = useQuery(geminiPoliciesQuery)

  const syncMutation = useMutation({
    mutationFn: () => api.syncGeminiModels(),
    onSettled: () => refreshGeminiData(),
  })

  const isRefreshing = (modelsFetching || policiesFetching) && !syncMutation.isPending

  const models = modelsData?.models ?? []
  const lastSynced = models.length > 0
    ? fmtDatetimeShort(models[0].synced_at, tz)
    : null

  const policyMap = new Map<string, GeminiRateLimitPolicy>((policies ?? []).map(p => [p.model_name, p]))
  const globalDefault = policyMap.get('*')
  const syncedRows = [...models].sort((a, b) => a.model_name.localeCompare(b.model_name))

  function makeEditablePolicy(modelName: string): GeminiRateLimitPolicy {
    const existing = policyMap.get(modelName)
    if (existing) return existing
    return {
      id: '',
      model_name: modelName,
      rpm_limit: globalDefault?.rpm_limit ?? 0,
      rpd_limit: globalDefault?.rpd_limit ?? 0,
      available_on_free_tier: globalDefault?.available_on_free_tier ?? true,
      updated_at: '',
    }
  }

  const tableLoading = modelsLoading || policiesLoading
  const hasContent = !!globalDefault || models.length > 0

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold text-text-bright flex items-center gap-2">
          <RotateCcw className="h-4 w-4 text-accent-gpu" />
          {t('providers.gemini.syncSection')}
        </h2>
        <p className="text-sm text-muted-foreground mt-0.5">{t('providers.gemini.syncSectionDesc')}</p>
      </div>

      <Card>
        <CardContent className="p-4 space-y-4">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <p className="text-sm font-medium">{t('providers.gemini.syncKey')}</p>
              <p className="font-mono text-xs text-muted-foreground mt-0.5 truncate">
                {syncConfig?.api_key_masked ?? <span className="italic">{t('providers.gemini.noSyncKey')}</span>}
              </p>
            </div>
            <Button size="sm" variant="outline" onClick={() => setShowSetKey(true)} className="shrink-0">
              {syncConfig?.api_key_masked ? t('common.edit') : t('providers.gemini.setSyncKey')}
            </Button>
          </div>

          <div className="flex items-center gap-3 flex-wrap">
            <Button size="sm" onClick={() => syncMutation.mutate()}
              disabled={syncMutation.isPending || !syncConfig?.api_key_masked}
              className="gap-1.5">
              <RotateCcw className={syncMutation.isPending ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {syncMutation.isPending ? t('common.syncing') : t('providers.gemini.syncNow')}
            </Button>
            <Button size="sm" variant="outline" onClick={refreshGeminiData}
              disabled={isRefreshing || syncMutation.isPending}
              className="gap-1.5">
              <RefreshCw className={isRefreshing ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
              {t('common.refresh')}
            </Button>
            {lastSynced && (
              <span className="text-xs text-muted-foreground">
                {t('providers.gemini.lastSynced')}: {lastSynced}
              </span>
            )}
            {syncMutation.data && (
              <span className="text-xs text-status-success-fg">
                ✓ {syncMutation.data.count} {t('providers.gemini.globalModels').toLowerCase()}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-accent-gpu" />
          <h3 className="text-sm font-semibold text-text-bright">{t('providers.gemini.rateLimitPolicies')}</h3>
        </div>
        <p className="text-sm text-muted-foreground">
          {t('providers.gemini.rateLimitDesc')}
          {' '}{t('providers.gemini.globalFallbackHint')}
        </p>

        {tableLoading && (
          <div className="flex h-16 items-center justify-center text-muted-foreground text-sm animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!tableLoading && !hasContent && (
          <Card className="border-dashed">
            <CardContent className="p-6 text-center text-muted-foreground text-sm">
              {t('providers.gemini.noGlobalModels')}
            </CardContent>
          </Card>
        )}

        {!tableLoading && hasContent && (
          <DataTable minWidth="600px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('providers.gemini.model')}</TableHead>
                <TableHead className="w-36">{t('providers.gemini.onFreeTier')}</TableHead>
                <TableHead className="w-24 text-right">{t('providers.gemini.rpm')}</TableHead>
                <TableHead className="w-24 text-right">{t('providers.gemini.rpd')}</TableHead>
                <TableHead className="w-40">{t('providers.gemini.lastUpdated')}</TableHead>
                <TableHead className="text-right w-20">{t('common.edit')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {syncedRows.map((m) => {
                const specific = policyMap.get(m.model_name)
                const isInherited = !specific
                const displayPolicy = specific ?? globalDefault
                return (
                  <TableRow key={m.model_name} className={isInherited ? 'opacity-60' : ''}>
                    <TableCell>
                      <span className="font-mono text-sm text-text-bright">{m.model_name}</span>
                    </TableCell>
                    <TableCell>
                      {isInherited ? (
                        <span className="text-xs text-muted-foreground italic">{t('providers.gemini.globalDefault')}</span>
                      ) : displayPolicy?.available_on_free_tier ? (
                        <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 text-[10px] px-1.5 py-0">
                          {t('providers.gemini.enabled')}
                        </Badge>
                      ) : (
                        <Badge variant="outline" className="bg-surface-code text-muted-foreground/70 border-border text-[10px] px-1.5 py-0">
                          {t('providers.gemini.paidOnly')}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums font-mono text-sm">
                      {displayPolicy && displayPolicy.rpm_limit > 0
                        ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpm_limit}</span>
                        : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-right tabular-nums font-mono text-sm">
                      {displayPolicy && displayPolicy.rpd_limit > 0
                        ? <span className={isInherited ? 'text-text-faint' : ''}>{displayPolicy.rpd_limit}</span>
                        : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {specific?.updated_at ? fmtDateOnly(specific.updated_at, tz) : <span className="text-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button variant="ghost" size="icon"
                        className="h-8 w-8 text-muted-foreground hover:text-status-info-fg hover:bg-status-info/10"
                        onClick={() => setEditingPolicy(makeEditablePolicy(m.model_name))}
                        title={t('providers.gemini.editPolicyTitle')}>
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </DataTable>
        )}
      </div>

      {showSetKey && (
        <SetSyncKeyModal current={syncConfig?.api_key_masked ?? null} onClose={() => setShowSetKey(false)} />
      )}
      {editingPolicy && (
        <EditPolicyModal policy={editingPolicy} onClose={() => setEditingPolicy(null)} />
      )}
    </div>
  )
}
