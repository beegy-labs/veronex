'use client'

import { useState } from 'react'
import { Wifi, WifiOff, AlertCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useTranslation } from '@/i18n'
import type { Provider } from '@/lib/types'

// ── Status Pill ─────────────────────────────────────────────────────────────────

export { StatusPill } from '@/components/status-pill'

// ── Helpers ────────────────────────────────────────────────────────────────────

export function extractHost(url: string): string {
  try { return new URL(url).host } catch { return url }
}

// ── Status badge ───────────────────────────────────────────────────────────────

export function StatusBadge({ status }: { status: Provider['status'] }) {
  const { t } = useTranslation()
  if (status === 'online') return (
    <Badge variant="outline" className="vds-bg-success/15 vds-text-success vds-border-success/30 vds-font-500 vds-whitespace-nowrap">
      <Wifi className="vds-h-3 vds-w-3 vds-mr-1.5 vds-flex-shrink-0" />{t('common.online')}
    </Badge>
  )
  if (status === 'degraded') return (
    <Badge variant="outline" className="vds-bg-warning/15 vds-text-warning vds-border-warning/30 vds-font-500 vds-whitespace-nowrap">
      <AlertCircle className="vds-h-3 vds-w-3 vds-mr-1.5 vds-flex-shrink-0" />{t('common.degraded')}
    </Badge>
  )
  return (
    <Badge variant="outline" className="vds-bg-surface-code vds-text-dim vds-border-subtle vds-font-500 vds-whitespace-nowrap">
      <WifiOff className="vds-h-3 vds-w-3 vds-mr-1.5 vds-flex-shrink-0" />{t('common.offline')}
    </Badge>
  )
}


// ── VRAM input with MiB / GiB toggle ──────────────────────────────────────────

export function VramInput({ valueMb, onChange, 'aria-label': ariaLabel }: { valueMb: string; onChange: (mb: string) => void; 'aria-label'?: string }) {
  const [unit, setUnit] = useState<'mb' | 'gb'>('gb')
  const mbNum = parseInt(valueMb) || 0
  const display = mbNum > 0
    ? (unit === 'gb' ? String(Math.round(mbNum / 1024 * 10) / 10) : String(mbNum))
    : ''

  function handleInput(raw: string) {
    if (!raw) { onChange(''); return }
    const n = parseFloat(raw)
    if (isNaN(n) || n < 0) return
    onChange(String(Math.round(unit === 'gb' ? n * 1024 : n)))
  }

  return (
    <div className="vds-flex">
      <Input type="number" min={0} step={unit === 'gb' ? 0.5 : 256}
        value={display} onChange={(e) => handleInput(e.target.value)}
        placeholder={unit === 'gb' ? 'e.g. 24' : 'e.g. 24576'}
        aria-label={ariaLabel}
        className="vds-rounded-r-none" />
      <Button type="button" variant={unit === 'mb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('mb')}
        className="vds-h-9 vds-px-2 vds-text-xs vds-rounded-none vds-border-l-0 vds-border-r-0 vds-flex-shrink-0">MiB</Button>
      <Button type="button" variant={unit === 'gb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('gb')}
        className="vds-h-9 vds-px-2 vds-text-xs vds-rounded-l-none vds-flex-shrink-0">GiB</Button>
    </div>
  )
}
