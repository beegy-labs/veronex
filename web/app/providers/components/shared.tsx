'use client'

import { useState } from 'react'
import { Wifi, WifiOff, AlertCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useTranslation } from '@/i18n'
import type { Provider } from '@/lib/types'

// ── Helpers ────────────────────────────────────────────────────────────────────

export function extractHost(url: string): string {
  try { return new URL(url).host } catch { return url }
}

// ── Status badge ───────────────────────────────────────────────────────────────

export function StatusBadge({ status }: { status: Provider['status'] }) {
  const { t } = useTranslation()
  if (status === 'online') return (
    <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30 font-medium">
      <Wifi className="h-3 w-3 mr-1.5" />{t('common.online')}
    </Badge>
  )
  if (status === 'degraded') return (
    <Badge variant="outline" className="bg-status-warning/15 text-status-warning-fg border-status-warning/30 font-medium">
      <AlertCircle className="h-3 w-3 mr-1.5" />{t('common.degraded')}
    </Badge>
  )
  return (
    <Badge variant="outline" className="bg-surface-code text-muted-foreground border-border font-medium">
      <WifiOff className="h-3 w-3 mr-1.5" />{t('common.offline')}
    </Badge>
  )
}


// ── VRAM input with MiB / GiB toggle ──────────────────────────────────────────

export function VramInput({ valueMb, onChange }: { valueMb: string; onChange: (mb: string) => void }) {
  const [unit, setUnit] = useState<'mb' | 'gb'>('mb')
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
    <div className="flex">
      <Input type="number" min={0} step={unit === 'gb' ? 0.5 : 256}
        value={display} onChange={(e) => handleInput(e.target.value)}
        placeholder={unit === 'gb' ? 'e.g. 24' : 'e.g. 24576'}
        className="rounded-r-none" />
      <Button type="button" variant={unit === 'mb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('mb')}
        className="h-9 px-2 text-xs rounded-none border-l-0 border-r-0 shrink-0">MiB</Button>
      <Button type="button" variant={unit === 'gb' ? 'secondary' : 'outline'}
        onClick={() => setUnit('gb')}
        className="h-9 px-2 text-xs rounded-l-none shrink-0">GiB</Button>
    </div>
  )
}
