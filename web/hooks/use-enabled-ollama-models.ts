'use client'

import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ollamaModelsQuery, globalModelSettingsQuery } from '@/lib/queries/providers'
import { isModelEnabled } from '@/lib/models'
import type { OllamaModelWithCount } from '@/lib/types'

/**
 * Model names that the operator has explicitly disabled via
 * "Providers → Ollama → Available Models" (`/v1/admin/global-model-settings`).
 *
 * Exposed as a separate hook so the sync page — which still *shows* disabled
 * models with a badge — can reuse the derivation without refetching the
 * paginated Ollama models list.
 */
export function useGlobalDisabledSet(): { disabledSet: ReadonlySet<string>; isLoading: boolean } {
  const { data, isLoading } = useQuery(globalModelSettingsQuery)
  const disabledSet = useMemo(
    () => new Set((data ?? []).filter((s) => !s.is_enabled).map((s) => s.model_name)),
    [data],
  )
  return { disabledSet, isLoading }
}

/**
 * SSOT for "which Ollama models should be shown as selectable in admin UI".
 *
 * A model is eligible only when:
 *   1. At least one carrying provider has it enabled
 *      (`is_enabled !== false` — see `isModelEnabled`).
 *   2. It is not in the global disable list
 *      (`globalModelSettings[].is_enabled === false`).
 *
 * Any UI that picks from Ollama models (vision selector, compression
 * selector, multi-turn allowlist, API test form, …) must use this hook —
 * otherwise a model disabled under "Providers → Ollama → Available Models"
 * would still appear in other pickers.
 */
export function useEnabledOllamaModels(params?: { limit?: number }): {
  models: OllamaModelWithCount[]
  isLoading: boolean
  disabledSet: ReadonlySet<string>
} {
  const { data: modelsData, isLoading: modelsLoading } = useQuery(
    ollamaModelsQuery({ limit: params?.limit ?? 200 }),
  )
  const { disabledSet, isLoading: globalLoading } = useGlobalDisabledSet()

  const models = useMemo(
    () => (modelsData?.models ?? []).filter((m) => isModelEnabled(m) && !disabledSet.has(m.model_name)),
    [modelsData?.models, disabledSet],
  )

  return { models, isLoading: modelsLoading || globalLoading, disabledSet }
}
