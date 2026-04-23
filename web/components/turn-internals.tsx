'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, Eye, Zap, Loader2 } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { turnInternalsQuery } from '@/lib/queries/conversations'
import { fmtCompact } from '@/lib/chart-theme'

interface TurnInternalsProps {
  convId: string
  jobId: string
}

export function TurnInternals({ convId, jobId }: TurnInternalsProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)

  const { data, isLoading, isError } = useQuery(turnInternalsQuery(convId, jobId, open))

  const hasData = data && (data.compressed || data.vision_analysis)

  return (
    <div className="mt-1">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 text-[11px] text-muted-foreground/60 hover:text-muted-foreground transition-colors"
      >
        {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        {t('conversations.internals')}
      </button>

      {open && (
        <div className="mt-1.5 pl-4 space-y-2">
          {isLoading && (
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" />
              {t('common.loading')}
            </div>
          )}
          {isError && (
            <p className="text-xs text-destructive">{t('common.error')}</p>
          )}
          {data && !hasData && (
            <p className="text-xs text-muted-foreground/60">{t('conversations.internalsEmpty')}</p>
          )}

          {data?.compressed && (
            <div className="space-y-1">
              <div className="flex items-center gap-1.5 text-xs font-medium">
                <Zap className="h-3 w-3 text-accent-power" />
                {t('conversations.compression')}
              </div>
              <div className="pl-4 grid grid-cols-2 gap-x-4 gap-y-0.5 text-[11px] text-muted-foreground font-mono">
                <span>{t('conversations.compressionModel')}</span>
                <span className="text-foreground truncate">{data.compressed.compression_model}</span>
                <span>{t('conversations.originalTokens')}</span>
                <span className="text-foreground">{fmtCompact(data.compressed.original_tokens)}</span>
                <span>{t('conversations.compressedTokens')}</span>
                <span className="text-foreground">{fmtCompact(data.compressed.compressed_tokens)}</span>
              </div>
              <div className="pl-4 mt-1 p-2 rounded bg-muted text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
                {data.compressed.summary}
              </div>
            </div>
          )}

          {data?.vision_analysis && (
            <div className="space-y-1">
              <div className="flex items-center gap-1.5 text-xs font-medium">
                <Eye className="h-3 w-3 text-status-info-fg" />
                {t('conversations.visionAnalysis')}
              </div>
              <div className="pl-4 grid grid-cols-2 gap-x-4 gap-y-0.5 text-[11px] text-muted-foreground font-mono">
                <span>{t('conversations.visionModel')}</span>
                <span className="text-foreground truncate">{data.vision_analysis.vision_model}</span>
                <span>{t('conversations.imageCount')}</span>
                <span className="text-foreground">{data.vision_analysis.image_count}</span>
                <span>{t('conversations.analysisTokens')}</span>
                <span className="text-foreground">{fmtCompact(data.vision_analysis.analysis_tokens)}</span>
              </div>
              <div className="pl-4 mt-1 p-2 rounded bg-muted text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
                {data.vision_analysis.analysis}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
