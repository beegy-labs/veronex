'use client'

import { useMemo, useState } from 'react'
import { Search } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { useTranslation } from '@/i18n'
import { useEnabledOllamaModels } from '@/hooks/use-enabled-ollama-models'

interface Props {
  selected: string[]
  onChange: (next: string[]) => void
  disabled?: boolean
  minParams?: number
  minCtx?: number
}

function parseParamsB(modelName: string): number | null {
  const m = modelName.match(/(\d+(?:\.\d+)?)\s*[bB](?:[:-]|$)/)
  return m ? parseFloat(m[1]) : null
}

export function MultiturnAllowedModelsSelector({
  selected, onChange, disabled, minParams, minCtx,
}: Props) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const { models, isLoading } = useEnabledOllamaModels()

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    return q ? models.filter((m) => m.model_name.toLowerCase().includes(q)) : models
  }, [models, search])

  const selectedSet = useMemo(() => new Set(selected), [selected])

  function toggle(name: string, checked: boolean) {
    if (checked) {
      if (!selectedSet.has(name)) onChange([...selected, name])
    } else {
      onChange(selected.filter((n) => n !== name))
    }
  }

  const allSelected = selected.length === 0

  return (
    <div className="vds-space-y-2">
      <div className="vds-flex vds-items-center vds-justify-between vds-gap-2">
        <div className="vds-relative vds-flex-1">
          <Search className="vds-absolute vds-left-2.5 vds-top-1/2 -translate-y-1/2 vds-h-3.5 vds-w-3.5 vds-text-dim vds-pointer-events-none" />
          <Input
            className="vds-h-8 vds-text-xs vds-pl-8"
            placeholder={t('providers.ollama.ollamaSearchModels')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            disabled={disabled}
          />
        </div>
        <span className="vds-text-2xs vds-text-dim vds-whitespace-nowrap">
          {allSelected
            ? t('providers.ollama.labMultiturnAllModels')
            : `${selected.length} / ${models.length}`}
        </span>
      </div>

      <div className="vds-max-h-64 vds-overflow-y-auto vds-divide-y vds-divide-border vds-rounded-md vds-border-1 vds-border-subtle">
        {isLoading && (
          <p className="vds-px-3 vds-py-3 vds-text-xs vds-text-dim">{t('common.loading')}</p>
        )}
        {!isLoading && filtered.length === 0 && (
          <p className="vds-px-3 vds-py-3 vds-text-xs vds-text-dim vds-italic">
            {search ? `${t('providers.ollama.noModelsMatch')} "${search}"` : t('providers.ollama.ollamaNoSync')}
          </p>
        )}
        {filtered.map((m) => {
          const params = parseParamsB(m.model_name)
          const paramsFail = minParams != null && params != null && params < minParams
          const ctxFail = minCtx != null && m.max_ctx != null && m.max_ctx > 0 && m.max_ctx < minCtx
          const gateFail = paramsFail || ctxFail
          const checked = selectedSet.has(m.model_name)
          return (
            <label
              key={m.model_name}
              className="vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2 vds-hover:bg-hover/40 vds-transition-colors vds-cursor-pointer"
            >
              <span className="vds-font-mono vds-text-xs vds-flex-1 vds-truncate">{m.model_name}</span>
              {params != null && (
                <Badge variant="outline" className={`vds-text-[10px] vds-px-1.5 vds-py-0 vds-tabular-nums ${paramsFail ? 'vds-border-warning/50 vds-text-warning' : ''}`}>
                  {params}B
                </Badge>
              )}
              {m.max_ctx != null && m.max_ctx > 0 && (
                <Badge variant="outline" className={`vds-text-[10px] vds-px-1.5 vds-py-0 vds-tabular-nums ${ctxFail ? 'vds-border-warning/50 vds-text-warning' : ''}`}>
                  {Math.floor(m.max_ctx / 1024)}k
                </Badge>
              )}
              {gateFail && (
                <Badge variant="outline" className="vds-text-[10px] vds-px-1.5 vds-py-0 vds-border-warning/50 vds-text-warning">
                  {t('providers.ollama.labMultiturnGateFail')}
                </Badge>
              )}
              <Switch
                checked={checked}
                disabled={disabled}
                onCheckedChange={(v) => toggle(m.model_name, v)}
                aria-label={m.model_name}
              />
            </label>
          )
        })}
      </div>
    </div>
  )
}
