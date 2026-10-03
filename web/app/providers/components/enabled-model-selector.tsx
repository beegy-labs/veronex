'use client'

import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { useEnabledLlamaServerModels } from '@/hooks/use-enabled-llama-server-models'

interface EnabledModelSelectorProps {
  value: string | null
  onChange: (v: string | null) => void
  disabled?: boolean
  preferVision?: boolean
}

export function EnabledModelSelector({ value, onChange, disabled, preferVision = false }: EnabledModelSelectorProps) {
  const { t } = useTranslation()
  const { models } = useEnabledLlamaServerModels()
  const visionModels = preferVision ? models.filter((model) => model.is_vision) : []
  const displayModels = visionModels.length > 0 ? visionModels : models

  return (
    <Select
      value={value ?? '__none__'}
      onValueChange={(v) => onChange(v === '__none__' ? null : v)}
      disabled={disabled}
    >
      <SelectTrigger className="vds-h-7 vds-text-xs vds-font-mono vds-w-full">
        <SelectValue placeholder={t('common.none')} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="__none__" className="vds-text-xs vds-text-dim">{t('common.none')}</SelectItem>
        {displayModels.map((m) => (
          <SelectItem key={m.model_name} value={m.model_name} className="vds-text-xs vds-font-mono">
            {m.model_name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
