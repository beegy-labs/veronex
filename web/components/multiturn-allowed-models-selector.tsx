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
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
          <Input
            className="h-8 text-xs pl-8"
            placeholder={t('providers.ollama.ollamaSearchModels')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            disabled={disabled}
          />
        </div>
        <span className="text-[11px] text-muted-foreground whitespace-nowrap">
          {allSelected
            ? t('providers.ollama.labMultiturnAllModels')
            : `${selected.length} / ${models.length}`}
        </span>
      </div>

      <div className="max-h-64 overflow-y-auto divide-y divide-border rounded-md border border-border">
        {isLoading && (
          <p className="px-3 py-3 text-xs text-muted-foreground">{t('common.loading')}</p>
        )}
        {!isLoading && filtered.length === 0 && (
          <p className="px-3 py-3 text-xs text-muted-foreground italic">
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
              className="flex items-center gap-3 px-3 py-2 hover:bg-muted/40 transition-colors cursor-pointer"
            >
              <span className="font-mono text-xs flex-1 truncate">{m.model_name}</span>
              {params != null && (
                <Badge variant="outline" className={`text-[10px] px-1.5 py-0 tabular-nums ${paramsFail ? 'border-status-warning/50 text-status-warning-fg' : ''}`}>
                  {params}B
                </Badge>
              )}
              {m.max_ctx != null && m.max_ctx > 0 && (
                <Badge variant="outline" className={`text-[10px] px-1.5 py-0 tabular-nums ${ctxFail ? 'border-status-warning/50 text-status-warning-fg' : ''}`}>
                  {Math.floor(m.max_ctx / 1024)}k
                </Badge>
              )}
              {gateFail && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-status-warning/50 text-status-warning-fg">
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
