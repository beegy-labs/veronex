'use client'

import { useState, useEffect, useOptimistic, startTransition } from 'react'
import {
  Languages, Clock, FlaskConical, Settings2,
} from 'lucide-react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { DEFAULT_MAX_IMAGES, MAX_IMAGES_LIMIT } from '@/lib/constants'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useTranslation } from '@/i18n'
import { i18n } from '@/i18n'
import { locales, localeLabels, localStorageKey, type Locale } from '@/i18n/config'
import { useLabSettings } from '@/components/lab-settings-provider'
import { useTimezone, type Timezone, PRESET_TIMEZONES, isValidTimezone } from '@/components/timezone-provider'
import { VisionModelSelector } from '@/components/vision-model-selector'
import { CompressionModelSelector } from '@/components/compression-model-selector'
import { api } from '@/lib/api'

function AllowedModelsInput({ labSettings, labLoading, setLabLoading, refetchLabSettings }: {
  labSettings: import('@/lib/types').LabSettings | null
  labLoading: boolean
  setLabLoading: (v: boolean) => void
  refetchLabSettings: () => void
}) {
  const { t } = useTranslation()
  const currentVal = (labSettings?.multiturn_allowed_models ?? []).join(', ')
  const [val, setVal] = useState(currentVal)
  const [dirty, setDirty] = useState(false)
  useEffect(() => { setVal((labSettings?.multiturn_allowed_models ?? []).join(', ')); setDirty(false) }, [labSettings?.multiturn_allowed_models])

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
        className="h-7 text-xs flex-1 font-mono"
        placeholder="qwen2.5:7b, mistral:7b"
        value={val}
        disabled={labLoading || labSettings === null}
        onChange={(e) => { setVal(e.target.value); setDirty(true) }}
        onKeyDown={(e) => { if (e.key === 'Enter' && dirty) save() }}
      />
      {dirty && (
        <Button size="sm" className="h-7 text-xs px-2" onClick={save} disabled={labLoading}>
          {t('common.save')}
        </Button>
      )}
    </div>
  )
}

function VisionModelInput({ labSettings, labLoading, setLabLoading, refetchLabSettings }: {
  labSettings: import('@/lib/types').LabSettings | null
  labLoading: boolean
  setLabLoading: (v: boolean) => void
  refetchLabSettings: () => void
}) {
  const { t } = useTranslation()
  const [val, setVal] = useState(labSettings?.vision_model ?? '')
  const [dirty, setDirty] = useState(false)
  // sync when labSettings loads
  useEffect(() => { setVal(labSettings?.vision_model ?? ''); setDirty(false) }, [labSettings?.vision_model])

  async function save() {
    setLabLoading(true)
    try {
      await api.patchLabSettings({ vision_model: val.trim() || null })
      await refetchLabSettings()
      setDirty(false)
    } catch { } finally { setLabLoading(false) }
  }

  return (
    <div className="flex gap-1.5">
      <Input
        className="h-7 text-xs flex-1 font-mono"
        placeholder="qwen3-vl:8b"
        value={val}
        disabled={labLoading || labSettings === null}
        onChange={(e) => { setVal(e.target.value); setDirty(true) }}
        onKeyDown={(e) => { if (e.key === 'Enter' && dirty) save() }}
      />
      {dirty && (
        <Button size="sm" className="h-7 text-xs px-2" onClick={save} disabled={labLoading}>
          {t('common.save')}
        </Button>
      )}
    </div>
  )
}

interface Props {
  open: boolean
  onClose: () => void
  resetToLocaleDefault: (locale: string) => void
}

export function NavSettingsDialog({ open, onClose, resetToLocaleDefault }: Props) {
  const { t } = useTranslation()
  const { tz, setTz } = useTimezone()
  const { labSettings, refetch: refetchLabSettings } = useLabSettings()

  const [locale, setLocale] = useState<Locale>(() => {
    const saved = localStorage.getItem(localStorageKey) as Locale | null
    if (saved && locales.includes(saved)) return saved
    const browser = navigator.language.slice(0, 2) as Locale
    return locales.includes(browser) ? browser : 'en'
  })
  const [showCustomTzInline, setShowCustomTzInline] = useState(false)
  const [customTzInput, setCustomTzInput] = useState('')
  const [customTzError, setCustomTzError] = useState(false)
  const [labLoading, setLabLoading] = useState(false)
  const [optCompressionEnabled, setOptCompressionEnabled] = useOptimistic(
    labSettings?.context_compression_enabled ?? false
  )
  const [optHandoffEnabled, setOptHandoffEnabled] = useOptimistic(
    labSettings?.handoff_enabled ?? false
  )

  const isPresetTz = PRESET_TIMEZONES.includes(tz as typeof PRESET_TIMEZONES[number])
  const tzSelectValue = isPresetTz ? tz : '__custom__'

  function changeLocale(next: Locale) {
    setLocale(next)
    localStorage.setItem(localStorageKey, next)
    i18n.changeLanguage(next)
    resetToLocaleDefault(next)
  }

  function handleClose() {
    setShowCustomTzInline(false)
    setCustomTzError(false)
    onClose()
  }

  function applyCustomTz() {
    if (isValidTimezone(customTzInput.trim())) {
      setTz(customTzInput.trim() as Timezone)
      setShowCustomTzInline(false)
    } else {
      setCustomTzError(true)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) handleClose() }}>
      <DialogContent className="max-w-xs">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings2 className="h-4 w-4 text-primary" />
            {t('common.settings')}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 pt-1">
          {/* Language row */}
          <div className="flex items-center gap-3">
            <Languages className="h-4 w-4 text-muted-foreground shrink-0" />
            <span className="text-sm text-muted-foreground flex-1">{t('common.language')}</span>
            <Select value={locale} onValueChange={(v) => changeLocale(v as Locale)}>
              <SelectTrigger className="h-8 w-36 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {locales.map((loc) => (
                  <SelectItem key={loc} value={loc} className="text-xs">
                    {localeLabels[loc]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Timezone row */}
          <div className="flex items-center gap-3">
            <Clock className="h-4 w-4 text-muted-foreground shrink-0" />
            <span className="text-sm text-muted-foreground flex-1">{t('common.timezone')}</span>
            <Select
              value={tzSelectValue}
              onValueChange={(v) => {
                if (v === '__custom__') {
                  setCustomTzInput(isPresetTz ? '' : tz)
                  setCustomTzError(false)
                  setShowCustomTzInline(true)
                } else {
                  setTz(v as Timezone)
                  setShowCustomTzInline(false)
                }
              }}
            >
              <SelectTrigger className="h-8 w-36 text-xs">
                {isPresetTz
                  ? <SelectValue />
                  : <span className="truncate">{tz.split('/').pop()}</span>
                }
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="UTC" className="text-xs">{t('common.utc')}</SelectItem>
                <SelectItem value="America/New_York" className="text-xs">{t('common.eastern')}</SelectItem>
                <SelectItem value="America/Chicago" className="text-xs">{t('common.central')}</SelectItem>
                <SelectItem value="America/Denver" className="text-xs">{t('common.mountain')}</SelectItem>
                <SelectItem value="America/Los_Angeles" className="text-xs">{t('common.pacific')}</SelectItem>
                <SelectItem value="Europe/London" className="text-xs">{t('common.london')}</SelectItem>
                <SelectItem value="Africa/Johannesburg" className="text-xs">{t('common.johannesburg')}</SelectItem>
                <SelectItem value="Asia/Seoul" className="text-xs">{t('common.kst')}</SelectItem>
                <SelectItem value="Asia/Tokyo" className="text-xs">{t('common.jst')}</SelectItem>
                <SelectItem value="Australia/Sydney" className="text-xs">{t('common.sydney')}</SelectItem>
                <SelectItem value="Pacific/Auckland" className="text-xs">{t('common.auckland')}</SelectItem>
                <SelectItem value="__custom__" className="text-xs">{t('common.custom')}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Lab features section */}
          <div className="border-t pt-3 mt-1">
            <div className="flex items-center gap-2 mb-2">
              <FlaskConical className="h-4 w-4 text-accent-power shrink-0" />
              <span className="text-sm font-medium flex-1">{t('common.labFeatures')}</span>
              <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-status-warning/15 text-status-warning-fg border border-status-warning/30 uppercase tracking-wide">
                Lab
              </span>
            </div>
            <p className="text-xs text-muted-foreground mb-3 pl-6">{t('common.labFeaturesDesc')}</p>

            {/* Gemini function calling */}
            <div className="pl-6 space-y-3">
              <div className="flex items-center justify-between gap-2">
                <div className="flex-1 min-w-0">
                  <p className="text-xs font-medium">{t('common.labGeminiFunctionCalling')}</p>
                  <p className="text-[11px] text-muted-foreground leading-snug mt-0.5">{t('common.labGeminiFunctionCallingDesc')}</p>
                </div>
                <Switch
                  checked={labSettings?.gemini_function_calling ?? false}
                  disabled={labLoading || labSettings === null}
                  aria-label={t('common.labGeminiFunctionCalling')}
                  onCheckedChange={async (checked) => {
                    setLabLoading(true)
                    try {
                      await api.patchLabSettings({ gemini_function_calling: checked })
                      await refetchLabSettings()
                    } catch {
                      // keep previous state on error
                    } finally {
                      setLabLoading(false)
                    }
                  }}
                />
              </div>

              {/* Image limits */}
              <div className="space-y-2">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex-1 min-w-0">
                    <p className="text-xs font-medium">{t('common.maxImagesPerRequest')}</p>
                    <p className="text-[11px] text-muted-foreground leading-snug mt-0.5">{t('common.maxImagesPerRequestDesc')}</p>
                  </div>
                  <Input
                    type="number"
                    min={0}
                    max={MAX_IMAGES_LIMIT}
                    className="w-20 h-7 text-xs text-center"
                    value={labSettings?.max_images_per_request ?? DEFAULT_MAX_IMAGES}
                    disabled={labLoading || labSettings === null}
                    onChange={async (e) => {
                      const val = parseInt(e.target.value, 10)
                      if (isNaN(val) || val < 0 || val > MAX_IMAGES_LIMIT) return
                      setLabLoading(true)
                      try {
                        await api.patchLabSettings({ max_images_per_request: val })
                        await refetchLabSettings()
                      } catch {
                        // keep previous state on error
                      } finally {
                        setLabLoading(false)
                      }
                    }}
                  />
                </div>
              </div>

              {/* Vision model */}
              <div className="space-y-1">
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

              {/* Compression settings */}
              <div className="border-t border-border/50 pt-2 mt-1">
                <p className="text-xs font-medium mb-2">{t('common.labCompression')}</p>
                <div className="space-y-2">
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-[11px] text-muted-foreground">{t('common.labCompressionEnabled')}</p>
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
                  <div className="space-y-1">
                    <p className="text-[11px] text-muted-foreground">{t('common.labCompressionModel')}</p>
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
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-[11px] text-muted-foreground">{t('common.labHandoffEnabled')}</p>
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
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-[11px] text-muted-foreground">{t('common.labHandoffThreshold')}</p>
                    <Input type="number" min={0.1} max={1} step={0.05} className="w-20 h-7 text-xs text-center"
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
                </div>
              </div>

              {/* Multi-turn requirements */}
              <div className="border-t border-border/50 pt-2 mt-1">
                <p className="text-xs font-medium mb-2">{t('common.labMultiturnReqs')}</p>
                <div className="space-y-2">
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-[11px] text-muted-foreground">{t('common.labMultiturnMinParams')}</p>
                    <Input type="number" min={0} max={1000} className="w-20 h-7 text-xs text-center"
                      value={labSettings?.multiturn_min_params ?? 7}
                      disabled={labLoading || labSettings === null}
                      onChange={async (e) => {
                        const val = parseInt(e.target.value, 10)
                        if (isNaN(val) || val < 0) return
                        setLabLoading(true)
                        try { await api.patchLabSettings({ multiturn_min_params: val }); await refetchLabSettings() }
                        catch { } finally { setLabLoading(false) }
                      }} />
                  </div>
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-[11px] text-muted-foreground">{t('common.labMultiturnMinCtx')}</p>
                    <Input type="number" min={0} className="w-24 h-7 text-xs text-center"
                      value={labSettings?.multiturn_min_ctx ?? 16384}
                      disabled={labLoading || labSettings === null}
                      onChange={async (e) => {
                        const val = parseInt(e.target.value, 10)
                        if (isNaN(val) || val < 0) return
                        setLabLoading(true)
                        try { await api.patchLabSettings({ multiturn_min_ctx: val }); await refetchLabSettings() }
                        catch { } finally { setLabLoading(false) }
                      }} />
                  </div>
                  <div className="space-y-1">
                    <p className="text-[11px] text-muted-foreground">{t('common.labMultiturnAllowedModels')}</p>
                    <p className="text-[10px] text-muted-foreground/70 leading-snug">{t('common.labMultiturnAllowedModelsDesc')}</p>
                    <AllowedModelsInput labSettings={labSettings} labLoading={labLoading} setLabLoading={setLabLoading} refetchLabSettings={refetchLabSettings} />
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* Custom IANA input */}
          {showCustomTzInline && (
            <div className="pl-7 space-y-2">
              <Input
                value={customTzInput}
                onChange={(e) => { setCustomTzInput(e.target.value); setCustomTzError(false) }}
                placeholder={t('common.customTimezonePlaceholder')}
                className="font-mono text-xs h-8"
                onKeyDown={(e) => { if (e.key === 'Enter') applyCustomTz() }}
              />
              <p className="text-xs text-muted-foreground">{t('common.customTimezoneHint')}</p>
              {customTzError && (
                <p className="text-xs text-destructive">{t('common.customTimezoneInvalid')}</p>
              )}
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs flex-1"
                  onClick={() => { setShowCustomTzInline(false); setCustomTzError(false) }}
                >
                  {t('common.cancel')}
                </Button>
                <Button
                  size="sm"
                  className="h-7 text-xs flex-1"
                  onClick={applyCustomTz}
                >
                  {t('common.save')}
                </Button>
              </div>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
