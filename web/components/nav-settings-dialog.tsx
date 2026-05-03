'use client'

import { useState, useOptimistic, startTransition } from 'react'
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
  const [optFunctionCalling, setOptFunctionCalling] = useOptimistic(
    labSettings?.gemini_function_calling ?? false
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
      <DialogContent className="vds-max-w-xs">
        <DialogHeader>
          <DialogTitle className="vds-flex vds-items-center vds-gap-2">
            <Settings2 className="vds-h-4 vds-w-4 vds-text-primary" />
            {t('common.settings')}
          </DialogTitle>
        </DialogHeader>

        <div className="vds-space-y-4 vds-pt-1">
          {/* Language row */}
          <div className="vds-flex vds-items-center vds-gap-3">
            <Languages className="vds-h-4 vds-w-4 vds-text-dim vds-flex-shrink-0" />
            <span className="vds-text-sm vds-text-dim vds-flex-1">{t('common.language')}</span>
            <Select value={locale} onValueChange={(v) => changeLocale(v as Locale)}>
              <SelectTrigger className="vds-h-8 vds-w-36 vds-text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {locales.map((loc) => (
                  <SelectItem key={loc} value={loc} className="vds-text-xs">
                    {localeLabels[loc]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Timezone row */}
          <div className="vds-flex vds-items-center vds-gap-3">
            <Clock className="vds-h-4 vds-w-4 vds-text-dim vds-flex-shrink-0" />
            <span className="vds-text-sm vds-text-dim vds-flex-1">{t('common.timezone')}</span>
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
              <SelectTrigger className="vds-h-8 vds-w-36 vds-text-xs">
                {isPresetTz
                  ? <SelectValue />
                  : <span className="vds-truncate">{tz.split('/').pop()}</span>
                }
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="UTC" className="vds-text-xs">{t('common.utc')}</SelectItem>
                <SelectItem value="America/New_York" className="vds-text-xs">{t('common.eastern')}</SelectItem>
                <SelectItem value="America/Chicago" className="vds-text-xs">{t('common.central')}</SelectItem>
                <SelectItem value="America/Denver" className="vds-text-xs">{t('common.mountain')}</SelectItem>
                <SelectItem value="America/Los_Angeles" className="vds-text-xs">{t('common.pacific')}</SelectItem>
                <SelectItem value="Europe/London" className="vds-text-xs">{t('common.london')}</SelectItem>
                <SelectItem value="Africa/Johannesburg" className="vds-text-xs">{t('common.johannesburg')}</SelectItem>
                <SelectItem value="Asia/Seoul" className="vds-text-xs">{t('common.kst')}</SelectItem>
                <SelectItem value="Asia/Tokyo" className="vds-text-xs">{t('common.jst')}</SelectItem>
                <SelectItem value="Australia/Sydney" className="vds-text-xs">{t('common.sydney')}</SelectItem>
                <SelectItem value="Pacific/Auckland" className="vds-text-xs">{t('common.auckland')}</SelectItem>
                <SelectItem value="__custom__" className="vds-text-xs">{t('common.custom')}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Lab features section */}
          <div className="vds-border-t-1 vds-pt-3 vds-mt-1">
            <div className="vds-flex vds-items-center vds-gap-2 vds-mb-2">
              <FlaskConical className="vds-h-4 vds-w-4 vds-text-accent-power vds-flex-shrink-0" />
              <span className="vds-text-sm vds-font-500 vds-flex-1">{t('common.labFeatures')}</span>
              <span className="vds-text-[10px] vds-font-600 vds-px-1.5 vds-py-0.5 vds-rounded vds-bg-warning/15 vds-text-warning vds-border-1 vds-border-warning/30 vds-uppercase vds-tracking-wide">
                Lab
              </span>
            </div>
            <p className="vds-text-xs vds-text-dim vds-mb-3 vds-pl-6">{t('common.labFeaturesDesc')}</p>

            {/* Gemini function calling (only truly global flag — Ollama-scoped features moved to /providers → Ollama tab → Lab) */}
            <div className="vds-pl-6 vds-space-y-3">
              <div className="vds-flex vds-items-center vds-justify-between vds-gap-2">
                <div className="vds-flex-1 vds-min-w-0">
                  <p className="vds-text-xs vds-font-500">{t('common.labGeminiFunctionCalling')}</p>
                  <p className="vds-text-2xs vds-text-dim vds-leading-snug vds-mt-0.5">{t('common.labGeminiFunctionCallingDesc')}</p>
                </div>
                <Switch
                  checked={optFunctionCalling}
                  disabled={labLoading || labSettings === null}
                  aria-label={t('common.labGeminiFunctionCalling')}
                  onCheckedChange={(checked) => {
                    startTransition(async () => {
                      setOptFunctionCalling(checked)
                      setLabLoading(true)
                      try {
                        await api.patchLabSettings({ gemini_function_calling: checked })
                        await refetchLabSettings()
                      } catch {
                        // keep previous state on error
                      } finally {
                        setLabLoading(false)
                      }
                    })
                  }}
                />
              </div>
            </div>
          </div>

          {/* Custom IANA input */}
          {showCustomTzInline && (
            <div className="vds-pl-7 vds-space-y-2">
              <Input
                value={customTzInput}
                onChange={(e) => { setCustomTzInput(e.target.value); setCustomTzError(false) }}
                placeholder={t('common.customTimezonePlaceholder')}
                className="vds-font-mono vds-text-xs vds-h-8"
                onKeyDown={(e) => { if (e.key === 'Enter') applyCustomTz() }}
              />
              <p className="vds-text-xs vds-text-dim">{t('common.customTimezoneHint')}</p>
              {customTzError && (
                <p className="vds-text-xs vds-text-destructive">{t('common.customTimezoneInvalid')}</p>
              )}
              <div className="vds-flex vds-gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  className="vds-h-7 vds-text-xs vds-flex-1"
                  onClick={() => { setShowCustomTzInline(false); setCustomTzError(false) }}
                >
                  {t('common.cancel')}
                </Button>
                <Button
                  size="sm"
                  className="vds-h-7 vds-text-xs vds-flex-1"
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
