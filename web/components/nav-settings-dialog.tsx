'use client'

import { useState } from 'react'
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
import { api } from '@/lib/api'

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
