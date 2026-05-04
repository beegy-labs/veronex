'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, Eye, Zap, Loader2, Wrench } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { turnInternalsQuery } from '@/lib/queries/conversations'
import { fmtCompact } from '@/lib/chart-theme'

interface TurnInternalsProps {
  convId: string
  jobId: string
  /** Open by default — used by the test panel where the user just ran a turn. */
  defaultOpen?: boolean
}

export function TurnInternals({ convId, jobId, defaultOpen = false }: TurnInternalsProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(defaultOpen)

  const { data, isLoading, isError } = useQuery(turnInternalsQuery(convId, jobId, open))

  const hasToolCalls = !!(data && data.tool_calls && data.tool_calls.length > 0)
  const hasData = data && (data.compressed || data.vision_analysis || hasToolCalls)

  return (
    <div className="vds-mt-1">
      <button
        onClick={() => setOpen((v) => !v)}
        className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-text-dim/60 vds-hover:text-dim vds-transition-colors"
      >
        {open ? <ChevronDown className="vds-h-3 vds-w-3" /> : <ChevronRight className="vds-h-3 vds-w-3" />}
        {t('conversations.internals')}
      </button>

      {open && (
        <div className="vds-mt-1.5 vds-pl-4 vds-space-y-2">
          {isLoading && (
            <div className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-dim">
              <Loader2 className="vds-h-3 vds-w-3 vds-animate-spin" />
              {t('common.loading')}
            </div>
          )}
          {isError && (
            <p className="vds-text-xs vds-text-destructive">{t('common.error')}</p>
          )}
          {data && !hasData && (
            <p className="vds-text-xs vds-text-dim/60">{t('conversations.internalsEmpty')}</p>
          )}

          {data?.compressed && (
            <div className="vds-space-y-1">
              <div className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-font-500">
                <Zap className="vds-h-3 vds-w-3 vds-text-accent-power" />
                {t('conversations.compression')}
              </div>
              <div className="vds-pl-4 vds-grid vds-grid-cols-2 vds-gap-x-4 vds-gap-y-0.5 vds-text-2xs vds-text-dim vds-font-mono">
                <span>{t('conversations.compressionModel')}</span>
                <span className="vds-text-primary vds-truncate">{data.compressed.compression_model}</span>
                <span>{t('conversations.originalTokens')}</span>
                <span className="vds-text-primary">{fmtCompact(data.compressed.original_tokens)}</span>
                <span>{t('conversations.compressedTokens')}</span>
                <span className="vds-text-primary">{fmtCompact(data.compressed.compressed_tokens)}</span>
              </div>
              <div className="vds-pl-4 vds-mt-1 vds-p-2 vds-rounded vds-bg-muted vds-text-2xs vds-font-mono vds-leading-relaxed vds-whitespace-pre-wrap vds-break-words vds-max-h-32 vds-overflow-y-auto">
                {data.compressed.summary}
              </div>
            </div>
          )}

          {data?.vision_analysis && (
            <div className="vds-space-y-1">
              <div className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-font-500">
                <Eye className="vds-h-3 vds-w-3 vds-text-info" />
                {t('conversations.visionAnalysis')}
              </div>
              <div className="vds-pl-4 vds-grid vds-grid-cols-2 vds-gap-x-4 vds-gap-y-0.5 vds-text-2xs vds-text-dim vds-font-mono">
                <span>{t('conversations.visionModel')}</span>
                <span className="vds-text-primary vds-truncate">{data.vision_analysis.vision_model}</span>
                <span>{t('conversations.imageCount')}</span>
                <span className="vds-text-primary">{data.vision_analysis.image_count}</span>
                <span>{t('conversations.analysisTokens')}</span>
                <span className="vds-text-primary">{fmtCompact(data.vision_analysis.analysis_tokens)}</span>
              </div>
              <div className="vds-pl-4 vds-mt-1 vds-p-2 vds-rounded vds-bg-muted vds-text-2xs vds-font-mono vds-leading-relaxed vds-whitespace-pre-wrap vds-break-words vds-max-h-32 vds-overflow-y-auto">
                {data.vision_analysis.analysis}
              </div>
            </div>
          )}

          {hasToolCalls && (
            <div className="vds-space-y-1">
              <div className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-font-500">
                <Wrench className="vds-h-3 vds-w-3 vds-text-info" />
                {t('conversations.toolCalls')}
                <span className="vds-text-dim/60 vds-font-mono vds-text-[10px]">
                  ({t('conversations.toolCallsBadge', { count: data!.tool_calls.length })})
                </span>
              </div>
              <div className="vds-pl-4 vds-space-y-1.5">
                {data!.tool_calls.map((tc, i) => (
                  <div
                    key={`tc-${tc.round}-${i}-${tc.namespaced_name}`}
                    className="vds-rounded vds-border-1 vds-border-subtle/50 vds-bg-muted/30 vds-px-2 vds-py-1.5 vds-text-2xs vds-font-mono"
                  >
                    <div className="vds-flex vds-items-center vds-gap-1.5 vds-flex-wrap">
                      <span className="vds-text-dim/70">
                        {t('conversations.toolCallsRound', { round: tc.round })}
                      </span>
                      <code className="vds-font-600 vds-text-primary">{tc.namespaced_name}</code>
                      <span
                        className={
                          tc.outcome === 'success'
                            ? 'vds-text-success'
                            : tc.outcome === 'cache_hit'
                              ? 'vds-text-accent-power'
                              : 'vds-text-error'
                        }
                      >
                        {tc.outcome}
                      </span>
                      {tc.cache_hit && (
                        <span className="vds-text-accent-power vds-text-[10px]">
                          {t('conversations.toolCallsCacheHit')}
                        </span>
                      )}
                      {tc.latency_ms != null && (
                        <span className="vds-text-dim/60 vds-ml-auto">
                          {t('conversations.toolCallsLatency', { ms: tc.latency_ms })}
                        </span>
                      )}
                    </div>
                    {tc.args !== null && tc.args !== undefined && (
                      <details className="vds-mt-1">
                        <summary className="vds-text-dim/60 vds-cursor-pointer vds-text-[10px] vds-select-none">
                          {t('conversations.toolCallsArgs')}
                        </summary>
                        <pre className="vds-mt-1 vds-p-1 vds-rounded vds-bg-page vds-text-[10px] vds-leading-snug vds-whitespace-pre-wrap vds-break-words vds-max-h-24 vds-overflow-y-auto">
                          {typeof tc.args === 'string' ? tc.args : JSON.stringify(tc.args, null, 2)}
                        </pre>
                      </details>
                    )}
                    <details className="vds-mt-1">
                      <summary className="vds-text-dim/60 vds-cursor-pointer vds-text-[10px] vds-select-none">
                        {t('conversations.toolCallsResult')}
                        {tc.result_bytes != null && (
                          <span className="vds-ml-1 vds-text-dim/40">
                            ({fmtCompact(tc.result_bytes)} B)
                          </span>
                        )}
                      </summary>
                      <pre className="vds-mt-1 vds-p-1 vds-rounded vds-bg-page vds-text-[10px] vds-leading-snug vds-whitespace-pre-wrap vds-break-words vds-max-h-32 vds-overflow-y-auto">
                        {tc.result_text ?? t('conversations.toolCallsNoResult')}
                      </pre>
                    </details>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
