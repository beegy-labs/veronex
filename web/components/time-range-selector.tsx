'use client'

import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Calendar, X } from 'lucide-react'
import { useTranslation } from '@/i18n'

export const TIME_OPTIONS = [
  { label: '1h',  hours: 1 },
  { label: '6h',  hours: 6 },
  { label: '24h', hours: 24 },
  { label: '7d',  hours: 168 },
  { label: '30d', hours: 720 },
] as const

export const TIME_LABEL_MAP = new Map<number, string>(
  TIME_OPTIONS.map(o => [o.hours, o.label]),
)

export interface TimeRange {
  hours: number
  from?: string
  to?: string
}

interface TimeRangeSelectorProps {
  value: TimeRange
  onChange: (value: TimeRange) => void
  className?: string
}

export function TimeRangeSelector({ value, onChange, className }: TimeRangeSelectorProps) {
  const { t } = useTranslation()
  const [showCustom, setShowCustom] = useState(false)
  const [customFrom, setCustomFrom] = useState('')
  const [customTo, setCustomTo] = useState('')

  const isPreset = !value.from && TIME_OPTIONS.some(o => o.hours === value.hours)

  function applyCustom() {
    if (!customFrom) return
    const from = new Date(customFrom)
    const to = customTo ? new Date(customTo + 'T23:59:59') : new Date()
    const diffMs = to.getTime() - from.getTime()
    if (diffMs <= 0) return
    const hours = Math.ceil(diffMs / (1000 * 60 * 60))
    onChange({ hours, from: from.toISOString(), to: to.toISOString() })
    setShowCustom(false)
  }

  function openCustom() {
    if (!showCustom) {
      // Pre-fill with current custom range if active
      if (value.from) {
        setCustomFrom(value.from.slice(0, 10))
        setCustomTo((value.to ?? '').slice(0, 10))
      }
      setShowCustom(true)
    } else {
      setShowCustom(false)
    }
  }

  function clearCustom() {
    onChange({ hours: 24 })
    setShowCustom(false)
    setCustomFrom('')
    setCustomTo('')
  }

  return (
    <div className={`flex items-center gap-1.5 flex-wrap${className ? ` ${className}` : ''}`}>
      {/* Preset buttons */}
      {TIME_OPTIONS.map((opt) => (
        <Button
          key={opt.hours}
          variant={isPreset && value.hours === opt.hours ? 'default' : 'outline'}
          size="sm"
          className="h-8 px-3 text-xs"
          onClick={() => { onChange({ hours: opt.hours }); setShowCustom(false) }}
        >
          {opt.label}
        </Button>
      ))}

      {/* Divider */}
      <div className="h-5 w-px bg-border mx-0.5" />

      {/* Custom button — shows active range label when a custom range is set */}
      {!showCustom && (
        value.from ? (
          <button
            onClick={openCustom}
            className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-primary bg-primary/10 text-primary text-xs font-medium hover:bg-primary/15 transition-colors"
          >
            <Calendar className="h-3 w-3 shrink-0" />
            <span>{value.from.slice(0, 10)}</span>
            <span className="text-primary/60">–</span>
            <span>{(value.to ?? '').slice(0, 10) || t('common.now')}</span>
            <X
              className="h-3 w-3 ml-0.5 opacity-60 hover:opacity-100"
              onClick={(e) => { e.stopPropagation(); clearCustom() }}
            />
          </button>
        ) : (
          <Button
            variant="outline"
            size="sm"
            className="h-8 px-3 text-xs gap-1.5"
            onClick={openCustom}
          >
            <Calendar className="h-3 w-3" />
            {t('common.custom')}
          </Button>
        )
      )}

      {/* Inline date picker — same row */}
      {showCustom && (
        <>
          <div className="flex items-center gap-1.5 rounded-md border bg-background px-2 h-8">
            <Input
              type="date"
              value={customFrom}
              onChange={(e) => setCustomFrom(e.target.value)}
              className="h-6 text-xs border-0 shadow-none p-0 w-32 focus-visible:ring-0"
            />
            <span className="text-xs text-muted-foreground select-none">–</span>
            <Input
              type="date"
              value={customTo}
              onChange={(e) => setCustomTo(e.target.value)}
              className="h-6 text-xs border-0 shadow-none p-0 w-32 focus-visible:ring-0"
            />
          </div>
          <Button size="sm" className="h-8 text-xs px-3" onClick={applyCustom} disabled={!customFrom}>
            {t('common.apply')}
          </Button>
          <Button size="sm" variant="ghost" className="h-8 w-8 p-0" onClick={() => setShowCustom(false)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </>
      )}
    </div>
  )
}
