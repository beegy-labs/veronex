'use client'

import { useQuery } from '@tanstack/react-query'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { ollamaModelsQuery } from '@/lib/queries/providers'
import { useTranslation } from '@/i18n'

interface VisionModelSelectorProps {
  value: string | null
  onChange: (v: string | null) => void
  disabled?: boolean
}

export function VisionModelSelector({ value, onChange, disabled }: VisionModelSelectorProps) {
  const { t } = useTranslation()
  const { data } = useQuery(ollamaModelsQuery({ limit: 200 }))
  // Show all models but mark vision-capable ones; fallback to all if none have is_vision flag
  const models = data?.models ?? []
  const visionModels = models.filter((m) => m.is_vision)
  const displayModels = visionModels.length > 0 ? visionModels : models

  return (
    <Select
      value={value ?? '__none__'}
      onValueChange={(v) => onChange(v === '__none__' ? null : v)}
      disabled={disabled}
    >
      <SelectTrigger className="h-7 text-xs font-mono w-full">
        <SelectValue placeholder={t('common.none')} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="__none__" className="text-xs text-muted-foreground">{t('common.none')}</SelectItem>
        {displayModels.map((m) => (
          <SelectItem key={m.model_name} value={m.model_name} className="text-xs font-mono">
            {m.model_name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
