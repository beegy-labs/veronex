'use client'

import { Button } from '@/components/ui/button'

export const TIME_OPTIONS = [
  { label: '24h', hours: 24 },
  { label: '7d',  hours: 168 },
  { label: '30d', hours: 720 },
] as const

interface TimeRangeSelectorProps {
  value: number
  onChange: (value: number) => void
  className?: string
}

export function TimeRangeSelector({ value, onChange, className }: TimeRangeSelectorProps) {
  return (
    <div className={`flex items-center gap-2 flex-wrap${className ? ` ${className}` : ''}`}>
      {TIME_OPTIONS.map((opt) => (
        <Button
          key={opt.hours}
          variant={value === opt.hours ? 'default' : 'outline'}
          size="sm"
          onClick={() => onChange(opt.hours)}
        >
          {opt.label}
        </Button>
      ))}
    </div>
  )
}
