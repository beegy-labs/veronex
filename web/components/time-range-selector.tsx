'use client'

import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Calendar } from 'lucide-react'
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

  const customLabel = value.from
    ? `${value.from.slice(0, 10)} ~ ${(value.to ?? '').slice(0, 10) || 'now'}`
    : null

  return (
    <div className={`flex items-center gap-1.5 flex-wrap${className ? ` ${className}` : ''}`}>
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
      <Button
        variant={showCustom || !isPreset ? 'secondary' : 'outline'}
        size="sm"
        className="h-8 px-3 text-xs"
        onClick={() => setShowCustom(v => !v)}
      >
        <Calendar className="h-3 w-3 mr-1" />
        {customLabel ?? t('common.custom')}
      </Button>
      {showCustom && (
        <div className="flex items-center gap-2 mt-1 w-full">
          <Input
            type="date"
            value={customFrom}
            onChange={(e) => setCustomFrom(e.target.value)}
            className="h-8 text-xs w-36"
          />
          <span className="text-xs text-muted-foreground">~</span>
          <Input
            type="date"
            value={customTo}
            onChange={(e) => setCustomTo(e.target.value)}
            className="h-8 text-xs w-36"
          />
          <Button size="sm" className="h-8 text-xs" onClick={applyCustom} disabled={!customFrom}>
            {t('common.apply')}
          </Button>
        </div>
      )}
    </div>
  )
}
