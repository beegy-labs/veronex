'use client'

import { X, Square, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'
import { renderWithMermaid } from '@/components/mermaid-block'
import type { Run } from '@/components/api-test-types'

interface ApiTestRunsProps {
  runs: Run[]
  activeRunId: number | null
  isAnyStreaming: boolean
  onSelectRun: (id: number) => void
  onCloseRun: (id: number) => void
  onStop: (id: number) => void
  onRerun: (run: Run) => void
}

export function ApiTestRuns({
  runs, activeRunId, isAnyStreaming,
  onSelectRun, onCloseRun, onStop, onRerun,
}: ApiTestRunsProps) {
  const { t } = useTranslation()
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null

  if (runs.length === 0) return null

  return (
    <div className="border-t border-border pt-4 space-y-3">
      {/* Tab strip */}
      <div className="flex items-center gap-1 border-b border-border pb-0 -mb-1 flex-wrap">
        {runs.map((run) => (
          <div
            key={run.id}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-t-md border border-b-0 cursor-pointer select-none transition-colors ${
              run.id === activeRunId
                ? 'bg-card border-border text-foreground'
                : 'bg-muted/40 border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/70'
            }`}
            onClick={() => onSelectRun(run.id)}
          >
            {run.status === 'streaming' && (
              <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse shrink-0" />
            )}
            {run.status === 'done' && (
              <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
            )}
            {run.status === 'error' && (
              <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
            )}
            <span>#{run.id}</span>
            <button
              type="button"
              className="ml-0.5 rounded hover:bg-destructive/20 hover:text-destructive p-0.5 -mr-1"
              onClick={(e) => { e.stopPropagation(); onCloseRun(run.id) }}
              title={t('common.close')}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ))}
      </div>

      {/* Active run output */}
      {activeRun && (
        <div className="pt-1 space-y-2">
          {/* Run controls */}
          {activeRun.status === 'streaming' && (
            <div className="flex items-center justify-between">
              <span className="flex items-center gap-1.5 text-xs text-status-info-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse" />
                {t('test.streaming')}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => onStop(activeRun.id)}
              >
                <Square className="h-3.5 w-3.5 mr-1.5" fill="currentColor" />
                {t('test.stop')}
              </Button>
            </div>
          )}

          {activeRun.status === 'done' && (
            <div className="flex items-center justify-between">
              <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30">
                {t('test.complete')}
              </Badge>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => onRerun(activeRun)}
                disabled={isAnyStreaming}
              >
                <RotateCcw className="h-3.5 w-3.5 mr-1.5" />
                {t('test.runAgain')}
              </Button>
            </div>
          )}

          {/* Output */}
          {(activeRun.text.length > 0 || activeRun.status === 'streaming') && (
            <div className="rounded-md border border-border bg-muted/20 p-3 min-h-[64px]">
              <div className="text-sm text-foreground font-mono leading-relaxed">
                {renderWithMermaid(activeRun.text, activeRun.status === 'streaming')}
              </div>
            </div>
          )}

          {/* Error */}
          {activeRun.status === 'error' && (
            <div className="rounded-md border border-status-error/30 bg-status-error/5 p-3">
              <p className="font-semibold text-sm text-status-error-fg">{t('test.errorTitle')}</p>
              <p className="text-sm mt-1 text-status-error-fg/80">{activeRun.errorMsg}</p>
            </div>
          )}

          {/* Prompt snapshot for context */}
          <p className="text-xs text-muted-foreground truncate">
            <span className="font-medium">{activeRun.model}</span>
            {' · '}
            <span className="opacity-70">{activeRun.prompt.slice(0, 80)}{activeRun.prompt.length > 80 ? '…' : ''}</span>
          </p>
        </div>
      )}
    </div>
  )
}
