'use client'

import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { useEnabledOllamaModels } from '@/hooks/use-enabled-ollama-models'

interface CompressionModelSelectorProps {
  value: string | null
  onChange: (v: string | null) => void
  disabled?: boolean
}

export function CompressionModelSelector({ value, onChange, disabled }: CompressionModelSelectorProps) {
  const { t } = useTranslation()
  const { models } = useEnabledOllamaModels()

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
        {models.map((m) => (
          <SelectItem key={m.model_name} value={m.model_name} className="text-xs font-mono">
            {m.model_name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
