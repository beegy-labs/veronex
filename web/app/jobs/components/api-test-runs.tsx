'use client'

import { memo } from 'react'
import { X, Square, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'
import { renderWithMermaid } from '@/components/mermaid-block'
import { CopyButton } from '@/components/copy-button'
import type { Run } from './api-test-types'
import { STATUS_STYLES } from '@/lib/constants'

interface ApiTestRunsProps {
  runs: Run[]
  activeRunId: number | null
  isAnyStreaming: boolean
  onSelectRun: (id: number) => void
  onCloseRun: (id: number) => void
  onStop: (id: number) => void
  onRerun: (run: Run) => void
}

export const ApiTestRuns = memo(function ApiTestRuns({
  runs, activeRunId, isAnyStreaming,
  onSelectRun, onCloseRun, onStop, onRerun,
}: ApiTestRunsProps) {
  const { t } = useTranslation()
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null

  if (runs.length === 0) return null

  return (
    <div className="vds-border-t-1 vds-border-subtle vds-pt-4 vds-space-y-3">
      {/* Tab strip */}
      <div className="vds-flex vds-items-center vds-gap-1 vds-border-b-1 vds-border-subtle vds-pb-0 vds--mb-1 vds-flex-wrap">
        {runs.map((run) => (
          <div
            key={run.id}
            role="button"
            tabIndex={0}
            className={`vds-flex vds-items-center vds-gap-1.5 vds-px-3 vds-py-1.5 vds-text-xs vds-font-500 vds-rounded-t-md vds-border-1 vds-border-b-0 vds-cursor-pointer vds-select-none vds-transition-colors ${
 run.id === activeRunId
 ? 'vds-bg-card vds-border-subtle vds-text-primary'
 : 'vds-bg-muted/40 vds-border-transparent vds-text-dim vds-hover:text-primary vds-hover:bg-hover/70'
 }`}
            onClick={() => onSelectRun(run.id)}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectRun(run.id) } }}
          >
            {run.status === 'streaming' && (
              <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-info-bg-fg vds-animate-pulse vds-flex-shrink-0" />
            )}
            {run.status === 'done' && (
              <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-flex-shrink-0" />
            )}
            {run.status === 'error' && (
              <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-flex-shrink-0" />
            )}
            <span>#{run.id}</span>
            <button
              type="button"
              aria-label={t('common.close')}
              className="vds-ml-0.5 vds-rounded vds-hover:bg-destructive/20 vds-hover:text-destructive vds-p-0.5 vds--mr-1"
              onClick={(e) => { e.stopPropagation(); onCloseRun(run.id) }}
              title={t('common.close')}
            >
              <X className="vds-h-3 vds-w-3" />
            </button>
          </div>
        ))}
      </div>

      {/* Active run output */}
      {activeRun && (
        <div className="vds-pt-1 vds-space-y-2">
          {/* Run controls */}
          {activeRun.status === 'streaming' && (
            <div className="vds-flex vds-items-center vds-justify-between">
              <span className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-info">
                <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-info-bg-fg vds-animate-pulse" />
                {t('test.streaming')}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => onStop(activeRun.id)}
              >
                <Square className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" fill="currentColor" />
                {t('test.stop')}
              </Button>
            </div>
          )}

          {activeRun.status === 'done' && (
            <div className="vds-flex vds-items-center vds-justify-between">
              <Badge variant="outline" className={`vds-whitespace-nowrap ${STATUS_STYLES['completed']}`}>
                {t('test.complete')}
              </Badge>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => onRerun(activeRun)}
                disabled={isAnyStreaming}
              >
                <RotateCcw className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
                {t('test.runAgain')}
              </Button>
            </div>
          )}

          {/* Attached images */}
          {activeRun.images && activeRun.images.length > 0 && (
            <div className="vds-flex vds-flex-wrap vds-gap-2">
              {activeRun.images.map((b64, i) => (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  key={b64.slice(0, 16)}
                  src={`data:image/jpeg;base64,${b64}`}
                  alt={`image-${i + 1}`}
                  className="vds-h-12 vds-w-12 vds-sm:h-16 vds-sm:w-16 vds-rounded-md vds-object-cover vds-border-1 vds-border-subtle"
                />
              ))}
            </div>
          )}

          {/* MCP tool-call timeline (SDD §7) — append-only as the SSE stream
              progresses. Surfaced even after `status === 'done'` so users can
              audit which tools were used. */}
          {activeRun.toolCalls.length > 0 && (
            <div className="vds-rounded-md vds-border-1 vds-border-subtle vds-bg-muted/10 vds-p-2 vds-space-y-1">
              <div className="vds-text-xs vds-font-600 vds-text-dim vds-tracking-wide">
                {t('test.toolsUsed')}
              </div>
              <ol className="vds-space-y-1 list-none">
                {activeRun.toolCalls.map((tc, i) => (
                  <li key={`${tc.name}-${i}`} className="vds-flex vds-items-center vds-gap-2 vds-text-xs">
                    <span className="vds-inline-flex vds-items-center vds-justify-center vds-h-4 vds-w-4 vds-rounded-full vds-bg-primary/10 vds-text-primary vds-text-[10px] vds-font-mono">
                      {i + 1}
                    </span>
                    <code className="vds-font-mono">{tc.name}</code>
                  </li>
                ))}
              </ol>
            </div>
          )}

          {/* Output */}
          {(activeRun.text.length > 0 || activeRun.status === 'streaming') && (
            <div className="vds-relative vds-rounded-md vds-border-1 vds-border-subtle vds-bg-muted/20 vds-p-3 vds-min-h-16 vds-group">
              {activeRun.text.length > 0 && (
                <div className="vds-absolute vds-top-1.5 vds-right-1.5 vds-opacity-0 vds-group-hover:opacity-100 vds-transition-opacity">
                  <CopyButton text={activeRun.text} />
                </div>
              )}
              <div className="vds-text-sm vds-text-primary vds-font-mono vds-leading-relaxed">
                {renderWithMermaid(activeRun.text, activeRun.status === 'streaming')}
              </div>
            </div>
          )}

          {/* Error */}
          {activeRun.status === 'error' && (
            <div className="vds-rounded-md vds-border-1 vds-border-error/30 vds-bg-error/5 vds-p-3">
              <p className="vds-font-600 vds-text-sm vds-text-error">{t('test.errorTitle')}</p>
              <p className="vds-text-sm vds-mt-1 vds-text-error/80">{activeRun.errorMsg}</p>
            </div>
          )}

          {/* Prompt snapshot for context */}
          <p className="vds-text-xs vds-text-dim vds-truncate">
            <span className="vds-font-500">{activeRun.model}</span>
            {' · '}
            <span className="vds-opacity-70">{activeRun.prompt.slice(0, 80)}{activeRun.prompt.length > 80 ? '…' : ''}</span>
          </p>
        </div>
      )}
    </div>
  )
})
