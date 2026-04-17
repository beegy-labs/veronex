'use client'

import { useState, useEffect, useOptimistic, startTransition } from 'react'
import { FlaskConical } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'
import { useLabSettings } from '@/components/lab-settings-provider'
import { VisionModelSelector } from '@/components/vision-model-selector'
import { CompressionModelSelector } from '@/components/compression-model-selector'
import { DEFAULT_MAX_IMAGES, MAX_IMAGES_LIMIT } from '@/lib/constants'
import { api } from '@/lib/api'
import type { LabSettings } from '@/lib/types'

function AllowedModelsInput({ labSettings, labLoading, setLabLoading, refetchLabSettings }: {
  labSettings: LabSettings | null
  labLoading: boolean
  setLabLoading: (v: boolean) => void
  refetchLabSettings: () => void
}) {
  const { t } = useTranslation()
  const [val, setVal] = useState((labSettings?.multiturn_allowed_models ?? []).join(', '))
  const [dirty, setDirty] = useState(false)
  useEffect(() => {
    setVal((labSettings?.multiturn_allowed_models ?? []).join(', '))
    setDirty(false)
  }, [labSettings?.multiturn_allowed_models])

  async function save() {
    setLabLoading(true)
    try {
      const models = val.split(',').map(s => s.trim()).filter(Boolean)
      await api.patchLabSettings({ multiturn_allowed_models: models })
      await refetchLabSettings()
      setDirty(false)
    } catch { } finally { setLabLoading(false) }
  }

  return (
    <div className="flex gap-1.5">
      <Input
        className="h-8 text-xs flex-1 font-mono"
        placeholder={t('common.labAllowedModelsPlaceholder')}
        value={val}
        disabled={labLoading || labSettings === null}
        onChange={(e) => { setVal(e.target.value); setDirty(true) }}
        onKeyDown={(e) => { if (e.key === 'Enter' && dirty) save() }}
      />
      {dirty && (
        <Button size="sm" className="h-8 text-xs px-3" onClick={save} disabled={labLoading}>
          {t('common.save')}
        </Button>
      )}
    </div>
  )
}

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

          <div className="flex items-center justify-between gap-3">
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium">{t('common.maxImagesPerRequest')}</p>
              <p className="text-[11px] text-muted-foreground leading-snug mt-0.5">{t('common.maxImagesPerRequestDesc')}</p>
            </div>
            <Input
              type="number"
              min={0}
              max={MAX_IMAGES_LIMIT}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.max_images_per_request ?? DEFAULT_MAX_IMAGES}
              disabled={labLoading || labSettings === null}
              onChange={async (e) => {
                const val = parseInt(e.target.value, 10)
                if (isNaN(val) || val < 0 || val > MAX_IMAGES_LIMIT) return
                setLabLoading(true)
                try {
                  await api.patchLabSettings({ max_images_per_request: val })
                  await refetchLabSettings()
                } catch { } finally { setLabLoading(false) }
              }}
            />
          </div>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labVisionModel')}</p>
            <p className="text-[11px] text-muted-foreground leading-snug">{t('common.labVisionModelDesc')}</p>
            <VisionModelSelector
              value={labSettings?.vision_model ?? null}
              disabled={labLoading || labSettings === null}
              onChange={async (v) => {
                setLabLoading(true)
                try { await api.patchLabSettings({ vision_model: v }); await refetchLabSettings() }
                catch { } finally { setLabLoading(false) }
              }}
            />
          </div>
        </CardContent>
      </Card>

      {/* ── Context compression ────────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <h3 className="text-sm font-semibold">{t('common.labCompression')}</h3>

          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium">{t('common.labCompressionEnabled')}</p>
            <Switch
              checked={optCompressionEnabled}
              disabled={labLoading || labSettings === null}
              onCheckedChange={(checked) => {
                startTransition(async () => {
                  setOptCompressionEnabled(checked)
                  setLabLoading(true)
                  try { await api.patchLabSettings({ context_compression_enabled: checked }); await refetchLabSettings() }
                  catch { } finally { setLabLoading(false) }
                })
              }}
            />
          </div>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labCompressionModel')}</p>
            <CompressionModelSelector
              value={labSettings?.compression_model ?? null}
              disabled={labLoading || labSettings === null}
              onChange={async (v) => {
                setLabLoading(true)
                try { await api.patchLabSettings({ compression_model: v }); await refetchLabSettings() }
                catch { } finally { setLabLoading(false) }
              }}
            />
          </div>

          <div className="flex items-center justify-between gap-3 pt-2 border-t border-border/50">
            <p className="text-xs font-medium">{t('common.labHandoffEnabled')}</p>
            <Switch
              checked={optHandoffEnabled}
              disabled={labLoading || labSettings === null}
              onCheckedChange={(checked) => {
                startTransition(async () => {
                  setOptHandoffEnabled(checked)
                  setLabLoading(true)
                  try { await api.patchLabSettings({ handoff_enabled: checked }); await refetchLabSettings() }
                  catch { } finally { setLabLoading(false) }
                })
              }}
            />
          </div>

          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium">{t('common.labHandoffThreshold')}</p>
            <Input
              type="number"
              min={0.1}
              max={1}
              step={0.05}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.handoff_threshold ?? 0.8}
              disabled={labLoading || labSettings === null}
              onChange={async (e) => {
                const v = parseFloat(e.target.value)
                if (isNaN(v) || v < 0.1 || v > 1) return
                setLabLoading(true)
                try { await api.patchLabSettings({ handoff_threshold: v }); await refetchLabSettings() }
                catch { } finally { setLabLoading(false) }
              }}
            />
          </div>
        </CardContent>
      </Card>

      {/* ── Multi-turn requirements ────────────────────────────────────────── */}
      <Card>
        <CardContent className="p-4 space-y-4">
          <h3 className="text-sm font-semibold">{t('common.labMultiturnReqs')}</h3>

          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium">{t('common.labMultiturnMinParams')}</p>
            <Input
              type="number"
              min={0}
              max={1000}
              className="w-24 h-8 text-xs text-center"
              value={labSettings?.multiturn_min_params ?? 7}
              disabled={labLoading || labSettings === null}
              onChange={async (e) => {
                const val = parseInt(e.target.value, 10)
                if (isNaN(val) || val < 0) return
                setLabLoading(true)
                try { await api.patchLabSettings({ multiturn_min_params: val }); await refetchLabSettings() }
                catch { } finally { setLabLoading(false) }
              }}
            />
          </div>

          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium">{t('common.labMultiturnMinCtx')}</p>
            <Input
              type="number"
              min={0}
              className="w-28 h-8 text-xs text-center"
              value={labSettings?.multiturn_min_ctx ?? 16384}
              disabled={labLoading || labSettings === null}
              onChange={async (e) => {
                const val = parseInt(e.target.value, 10)
                if (isNaN(val) || val < 0) return
                setLabLoading(true)
                try { await api.patchLabSettings({ multiturn_min_ctx: val }); await refetchLabSettings() }
                catch { } finally { setLabLoading(false) }
              }}
            />
          </div>

          <div className="space-y-1.5">
            <p className="text-xs font-medium">{t('common.labMultiturnAllowedModels')}</p>
            <p className="text-[11px] text-muted-foreground leading-snug">{t('common.labMultiturnAllowedModelsDesc')}</p>
            <AllowedModelsInput
              labSettings={labSettings}
              labLoading={labLoading}
              setLabLoading={setLabLoading}
              refetchLabSettings={refetchLabSettings}
            />
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
