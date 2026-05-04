'use client'

import { useState, useMemo } from 'react'
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
    <div className="vds-space-y-3">
      <h2 className="vds-text-base vds-font-600 vds-text-bright vds-flex vds-items-center vds-gap-2">
        <RefreshCw className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
        {t('providers.gemini.statusSyncSection')}
      </h2>

      <Card>
        <CardContent className="vds-p-4 vds-space-y-4">
          <p className="vds-text-sm vds-text-dim">{t('providers.gemini.statusSyncDesc')}</p>

          <div className="vds-flex vds-items-center vds-gap-3">
            <Button size="sm" onClick={() => syncMutation.mutate()} disabled={syncMutation.isPending} className="vds-gap-1.5">
              <RefreshCw className={syncMutation.isPending ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
              {syncMutation.isPending ? t('providers.gemini.syncingStatus') : t('providers.gemini.syncStatus')}
            </Button>
            {syncMutation.isSuccess && !syncMutation.isPending && (
              <span className="vds-text-xs vds-text-success">
                ✓ {t('providers.gemini.statusSyncDone')} — {onlineCount}/{results.length} {t('common.online').toLowerCase()}
              </span>
            )}
          </div>

          {syncMutation.isSuccess && results.length === 0 && (
            <p className="vds-text-xs vds-text-dim vds-italic">{t('providers.gemini.noStatusResults')}</p>
          )}

          {results.length > 0 && (
            <div className="vds-divide-y vds-divide-border vds-rounded-md vds-border-1 vds-border-subtle vds-overflow-hidden">
              {results.map((r) => (
                <div key={r.id} className="vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2.5">
                  <span className={statusDotCls(r.status)} />
                  <span className="vds-font-500 vds-text-sm vds-text-bright vds-flex-1 vds-truncate">{r.name}</span>
                  <span className={`vds-text-xs vds-font-500 ${statusResultCls(r.status)}`}>
                    {statusResultLabel(r.status, t)}
                  </span>
                  {r.error && (
                    <span className="vds-text-xs vds-text-error vds-truncate vds-max-w-[160px]" title={r.error}>
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

  const policyMap = useMemo(() => new Map<string, GeminiRateLimitPolicy>((policies ?? []).map(p => [p.model_name, p])), [policies])
  const globalDefault = policyMap.get('*')
  const syncedRows = useMemo(() => [...models].sort((a, b) => a.model_name.localeCompare(b.model_name)), [models])

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
    <div className="vds-space-y-4">
      <div>
        <h2 className="vds-text-base vds-font-600 vds-text-bright vds-flex vds-items-center vds-gap-2">
          <RotateCcw className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
          {t('providers.gemini.syncSection')}
        </h2>
        <p className="vds-text-sm vds-text-dim vds-mt-0.5">{t('providers.gemini.syncSectionDesc')}</p>
      </div>

      <Card>
        <CardContent className="vds-p-4 vds-space-y-4">
          <div className="vds-flex vds-items-center vds-justify-between vds-gap-4">
            <div className="vds-min-w-0">
              <p className="vds-text-sm vds-font-500">{t('providers.gemini.syncKey')}</p>
              <p className="vds-font-mono vds-text-xs vds-text-dim vds-mt-0.5 vds-truncate">
                {syncConfig?.api_key_masked ?? <span className="vds-italic">{t('providers.gemini.noSyncKey')}</span>}
              </p>
            </div>
            <Button size="sm" variant="outline" onClick={() => setShowSetKey(true)} className="vds-flex-shrink-0">
              {syncConfig?.api_key_masked ? t('common.edit') : t('providers.gemini.setSyncKey')}
            </Button>
          </div>

          <div className="vds-flex vds-items-center vds-gap-3 vds-flex-wrap">
            <Button size="sm" onClick={() => syncMutation.mutate()}
              disabled={syncMutation.isPending || !syncConfig?.api_key_masked}
              className="vds-gap-1.5">
              <RotateCcw className={syncMutation.isPending ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
              {syncMutation.isPending ? t('common.syncing') : t('providers.gemini.syncNow')}
            </Button>
            <Button size="sm" variant="outline" onClick={refreshGeminiData}
              disabled={isRefreshing || syncMutation.isPending}
              className="vds-gap-1.5">
              <RefreshCw className={isRefreshing ? 'vds-h-3.5 vds-w-3.5 vds-animate-spin' : 'vds-h-3.5 vds-w-3.5'} />
              {t('common.refresh')}
            </Button>
            {lastSynced && (
              <span className="vds-text-xs vds-text-dim">
                {t('providers.gemini.lastSynced')}: {lastSynced}
              </span>
            )}
            {syncMutation.data && (
              <span className="vds-text-xs vds-text-success">
                ✓ {syncMutation.data.count} {t('providers.gemini.globalModels').toLowerCase()}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="vds-space-y-3">
        <div className="vds-flex vds-items-center vds-gap-2">
          <ShieldCheck className="vds-h-4 vds-w-4 vds-text-accent-gpu" />
          <h3 className="vds-text-sm vds-font-600 vds-text-bright">{t('providers.gemini.rateLimitPolicies')}</h3>
        </div>
        <p className="vds-text-sm vds-text-dim">
          {t('providers.gemini.rateLimitDesc')}
          {' '}{t('providers.gemini.globalFallbackHint')}
        </p>

        {tableLoading && (
          <div className="vds-flex vds-h-16 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-animate-pulse">
            {t('common.loading')}
          </div>
        )}

        {!tableLoading && !hasContent && (
          <Card className="vds-border-dashed">
            <CardContent className="vds-p-6 vds-text-center vds-text-dim vds-text-sm">
              {t('providers.gemini.noGlobalModels')}
            </CardContent>
          </Card>
        )}

        {!tableLoading && hasContent && (
          <DataTable minWidth="600px">
            <TableHeader>
              <TableRow className="vds-hover:bg-transparent">
                <TableHead>{t('providers.gemini.model')}</TableHead>
                <TableHead className="vds-w-36">{t('providers.gemini.onFreeTier')}</TableHead>
                <TableHead className="vds-w-24 vds-text-right">{t('providers.gemini.rpm')}</TableHead>
                <TableHead className="vds-w-24 vds-text-right">{t('providers.gemini.rpd')}</TableHead>
                <TableHead className="vds-w-40">{t('providers.gemini.lastUpdated')}</TableHead>
                <TableHead className="vds-text-right vds-w-20">{t('common.edit')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {syncedRows.map((m) => {
                const specific = policyMap.get(m.model_name)
                const isInherited = !specific
                const displayPolicy = specific ?? globalDefault
                return (
                  <TableRow key={m.model_name} className={isInherited ? 'vds-opacity-60' : ''}>
                    <TableCell>
                      <span className="vds-font-mono vds-text-sm vds-text-bright">{m.model_name}</span>
                    </TableCell>
                    <TableCell>
                      {isInherited ? (
                        <span className="vds-text-xs vds-text-dim vds-italic">{t('providers.gemini.globalDefault')}</span>
                      ) : displayPolicy?.available_on_free_tier ? (
                        <Badge variant="outline" className="vds-bg-warning/15 vds-text-warning vds-border-warning/30 vds-text-[10px] vds-px-1.5 vds-py-0">
                          {t('providers.gemini.enabled')}
                        </Badge>
                      ) : (
                        <Badge variant="outline" className="vds-bg-surface-code vds-text-dim/70 vds-border-subtle vds-text-[10px] vds-px-1.5 vds-py-0">
                          {t('providers.gemini.paidOnly')}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums vds-font-mono vds-text-sm">
                      {displayPolicy && displayPolicy.rpm_limit > 0
                        ? <span className={isInherited ? 'vds-text-faint' : ''}>{displayPolicy.rpm_limit}</span>
                        : <span className="vds-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="vds-text-right vds-tabular-nums vds-font-mono vds-text-sm">
                      {displayPolicy && displayPolicy.rpd_limit > 0
                        ? <span className={isInherited ? 'vds-text-faint' : ''}>{displayPolicy.rpd_limit}</span>
                        : <span className="vds-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="vds-text-xs vds-text-dim">
                      {specific?.updated_at ? fmtDateOnly(specific.updated_at, tz) : <span className="vds-text-faint">—</span>}
                    </TableCell>
                    <TableCell className="vds-text-right">
                      <Button variant="ghost" size="icon"
                        className="vds-h-8 vds-w-8 vds-text-dim vds-hover:text-info vds-hover:bg-info/10"
                        aria-label={t('providers.gemini.editPolicyTitle')}
                        onClick={() => setEditingPolicy(makeEditablePolicy(m.model_name))}
                        title={t('providers.gemini.editPolicyTitle')}>
                        <Pencil className="vds-h-4 vds-w-4" />
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
