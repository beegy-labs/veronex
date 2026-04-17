'use client'

import { useState, useOptimistic, startTransition } from 'react'
import { FlaskConical } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'
import { useLabSettings } from '@/components/lab-settings-provider'
import { VisionModelSelector } from '@/components/vision-model-selector'
import { CompressionModelSelector } from '@/components/compression-model-selector'
import { MultiturnAllowedModelsSelector } from '@/components/multiturn-allowed-models-selector'
import { DEFAULT_MAX_IMAGES, MAX_IMAGES_LIMIT } from '@/lib/constants'
import { api } from '@/lib/api'

const BYTES_PER_MB = 1024 * 1024

export function OllamaLabSection() {
  const { t } = useTranslation()
  const { labSettings, refetch: refetchLabSettings } = useLabSettings()
  const [labLoading, setLabLoading] = useState(false)
  const [optCompressionEnabled, setOptCompressionEnabled] = useOptimistic(
    labSettings?.context_compression_enabled ?? false
  )
  const [optHandoffEnabled, setOptHandoffEnabled] = useOptimistic(
    labSettings?.handoff_enabled ?? false
  )

  async function patch<K extends keyof import('@/lib/types').PatchLabSettings>(
    key: K, value: import('@/lib/types').PatchLabSettings[K],
  ) {
    setLabLoading(true)
    try {
      await api.patchLabSettings({ [key]: value } as import('@/lib/types').PatchLabSettings)
      await refetchLabSettings()
    } catch { } finally { setLabLoading(false) }
  }

  const disabled = labLoading || labSettings === null

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <FlaskConical className="h-4 w-4 text-accent-power" />
        <h2 className="text-base font-semibold text-text-bright">{t('providers.ollama.labTitle')}</h2>
        <Badge variant="outline" className="text-[10px] px-1.5 py-0 bg-status-warning/15 text-status-warning-fg border-status-warning/30 uppercase">
          Lab
        </Badge>
      </div>
      <p className="text-xs text-muted-foreground">{t('providers.ollama.labDesc')}</p>

      {/* ── Image input ────────────────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <h3 className="text-sm font-semibold">{t('providers.ollama.labImageSection')}</h3>

          <Row
            label={t('common.maxImagesPerRequest')}
            desc={t('common.maxImagesPerRequestDesc')}
          >
            <Input
              type="number" min={0} max={MAX_IMAGES_LIMIT}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.max_images_per_request ?? DEFAULT_MAX_IMAGES}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 0 || v > MAX_IMAGES_LIMIT) return
                patch('max_images_per_request', v)
              }}
            />
          </Row>

          <Row
            label={t('providers.ollama.labMaxImageBytes')}
            desc={t('providers.ollama.labMaxImageBytesDesc')}
            suffix="MB"
          >
            <Input
              type="number" min={0.1} max={32} step={0.1}
              className="w-24 h-8 text-xs text-center"
              value={((labSettings?.max_image_b64_bytes ?? 2 * BYTES_PER_MB) / BYTES_PER_MB).toFixed(1)}
              disabled={disabled}
              onChange={(e) => {
                const mb = parseFloat(e.target.value)
                if (isNaN(mb) || mb <= 0 || mb > 32) return
                patch('max_image_b64_bytes', Math.round(mb * BYTES_PER_MB))
              }}
            />
          </Row>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labVisionModel')}</p>
            <p className="text-[11px] text-muted-foreground leading-snug">{t('common.labVisionModelDesc')}</p>
            <VisionModelSelector
              value={labSettings?.vision_model ?? null}
              disabled={disabled}
              onChange={(v) => patch('vision_model', v)}
            />
          </div>
        </CardContent>
      </Card>

      {/* ── Context compression ────────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <h3 className="text-sm font-semibold">{t('common.labCompression')}</h3>

          <Row label={t('common.labCompressionEnabled')}>
            <Switch
              checked={optCompressionEnabled}
              disabled={disabled}
              onCheckedChange={(checked) => {
                startTransition(async () => {
                  setOptCompressionEnabled(checked)
                  await patch('context_compression_enabled', checked)
                })
              }}
            />
          </Row>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labCompressionModel')}</p>
            <p className="text-[11px] text-muted-foreground leading-snug">{t('providers.ollama.labCompressionModelDesc')}</p>
            <CompressionModelSelector
              value={labSettings?.compression_model ?? null}
              disabled={disabled}
              onChange={(v) => patch('compression_model', v)}
            />
          </div>

          <Row
            label={t('providers.ollama.labContextBudgetRatio')}
            desc={t('providers.ollama.labContextBudgetRatioDesc')}
          >
            <Input
              type="number" min={0.1} max={1} step={0.05}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.context_budget_ratio ?? 0.6}
              disabled={disabled}
              onChange={(e) => {
                const v = parseFloat(e.target.value)
                if (isNaN(v) || v < 0.1 || v > 1) return
                patch('context_budget_ratio', v)
              }}
            />
          </Row>

          <Row
            label={t('providers.ollama.labCompressionTriggerTurns')}
            desc={t('providers.ollama.labCompressionTriggerTurnsDesc')}
          >
            <Input
              type="number" min={1} max={20}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.compression_trigger_turns ?? 1}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 1 || v > 20) return
                patch('compression_trigger_turns', v)
              }}
            />
          </Row>

          <Row
            label={t('providers.ollama.labRecentVerbatim')}
            desc={t('providers.ollama.labRecentVerbatimDesc')}
          >
            <Input
              type="number" min={0} max={20}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.recent_verbatim_window ?? 1}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 0 || v > 20) return
                patch('recent_verbatim_window', v)
              }}
            />
          </Row>

          <Row
            label={t('providers.ollama.labCompressionTimeout')}
            desc={t('providers.ollama.labCompressionTimeoutDesc')}
            suffix="s"
          >
            <Input
              type="number" min={1} max={300}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.compression_timeout_secs ?? 10}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 1 || v > 300) return
                patch('compression_timeout_secs', v)
              }}
            />
          </Row>

          <div className="pt-3 border-t border-border/50 space-y-4">
            <Row label={t('common.labHandoffEnabled')}>
              <Switch
                checked={optHandoffEnabled}
                disabled={disabled}
                onCheckedChange={(checked) => {
                  startTransition(async () => {
                    setOptHandoffEnabled(checked)
                    await patch('handoff_enabled', checked)
                  })
                }}
              />
            </Row>

            <Row
              label={t('common.labHandoffThreshold')}
              desc={t('providers.ollama.labHandoffThresholdDesc')}
            >
              <Input
                type="number" min={0.1} max={1} step={0.05}
                className="w-24 h-8 text-xs text-center"
                value={labSettings?.handoff_threshold ?? 0.85}
                disabled={disabled}
                onChange={(e) => {
                  const v = parseFloat(e.target.value)
                  if (isNaN(v) || v < 0.1 || v > 1) return
                  patch('handoff_threshold', v)
                }}
              />
            </Row>
          </div>
        </CardContent>
      </Card>

      {/* ── Multi-turn requirements ────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <h3 className="text-sm font-semibold">{t('common.labMultiturnReqs')}</h3>

          <Row
            label={t('common.labMultiturnMinParams')}
            desc={t('providers.ollama.labMultiturnMinParamsDesc')}
            suffix="B"
          >
            <Input
              type="number" min={0} max={1000}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.multiturn_min_params ?? 7}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 0) return
                patch('multiturn_min_params', v)
              }}
            />
          </Row>

          <Row
            label={t('common.labMultiturnMinCtx')}
            desc={t('providers.ollama.labMultiturnMinCtxDesc')}
          >
            <Input
              type="number" min={0}
              className="w-28 h-8 text-xs text-center"
              value={labSettings?.multiturn_min_ctx ?? 16384}
              disabled={disabled}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10)
                if (isNaN(v) || v < 0) return
                patch('multiturn_min_ctx', v)
              }}
            />
          </Row>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labMultiturnAllowedModels')}</p>
            <p className="text-[11px] text-muted-foreground leading-snug">{t('providers.ollama.labMultiturnAllowedModelsDesc')}</p>
            <MultiturnAllowedModelsSelector
              selected={labSettings?.multiturn_allowed_models ?? []}
              disabled={disabled}
              minParams={labSettings?.multiturn_min_params}
              minCtx={labSettings?.multiturn_min_ctx}
              onChange={(next) => patch('multiturn_allowed_models', next)}
            />
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function Row({ label, desc, suffix, children }: {
  label: string
  desc?: string
  suffix?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="flex-1 min-w-0">
        <p className="text-xs font-medium">{label}</p>
        {desc && <p className="text-[11px] text-muted-foreground leading-snug mt-0.5">{desc}</p>}
      </div>
      <div className="flex items-center gap-1.5 shrink-0">
        {children}
        {suffix && <span className="text-[11px] text-muted-foreground">{suffix}</span>}
      </div>
    </div>
  )
}
