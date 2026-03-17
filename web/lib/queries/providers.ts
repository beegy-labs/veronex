import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, STALE_TIME_FAST, REFETCH_INTERVAL_FAST } from '@/lib/constants'

// ── LLM providers list ─────────────────────────────────────────────────────────

export const providersQuery = queryOptions({
  queryKey: ['providers'] as const,
  queryFn: () => api.providers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
})

// ── Provider-specific ──────────────────────────────────────────────────────────

export const providerModelsQuery = (providerId: string) => queryOptions({
  queryKey: ['provider-models', providerId] as const,
  queryFn: () => api.providerModels(providerId),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

export const providerKeyQuery = (providerId: string) => queryOptions({
  queryKey: ['provider-key', providerId] as const,
  queryFn: () => api.providerKey(providerId),
  enabled: false,
})

export const ollamaModelProvidersQuery = (modelName: string) => queryOptions({
  queryKey: ['ollama-model-providers', modelName] as const,
  queryFn: () => api.ollamaModelProviders(modelName),
  staleTime: STALE_TIME_FAST,
})

export const selectedModelsQuery = (providerId: string) => queryOptions({
  queryKey: ['selected-models', providerId] as const,
  queryFn: () => api.getSelectedModels(providerId),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

// ── Ollama ────────────────────────────────────────────────────────────────────

export const ollamaModelsQuery = queryOptions({
  queryKey: ['ollama-models'] as const,
  queryFn: () => api.ollamaModels(),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

export const ollamaSyncStatusQuery = queryOptions({
  queryKey: ['ollama-sync-status'] as const,
  queryFn: () => api.ollamaSyncStatus(),
  staleTime: 4_900,
  retry: false,
})

// ── Query key constants (for invalidation) ──────────────────────────────────

export const GEMINI_QUERY_KEYS = {
  syncConfig:     ['gemini-sync-config'] as const,
  models:         ['gemini-models'] as const,
  policies:       ['gemini-policies'] as const,
  selectedModels: ['selected-models'] as const,
} as const

// ── Gemini ────────────────────────────────────────────────────────────────────

export const geminiPoliciesQuery = queryOptions({
  queryKey: ['gemini-policies'] as const,
  queryFn: () => api.geminiPolicies(),
  staleTime: Infinity,
  retry: false,
})

export const geminiModelsQuery = queryOptions({
  queryKey: ['gemini-models'] as const,
  queryFn: () => api.geminiModels(),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

export const geminiSyncConfigQuery = queryOptions({
  queryKey: ['gemini-sync-config'] as const,
  queryFn: () => api.geminiSyncConfig(),
  staleTime: Infinity,
  retry: false,
})

// ── Capacity ──────────────────────────────────────────────────────────────────

export const capacityQuery = queryOptions({
  queryKey: ['capacity'] as const,
  queryFn: () => api.capacity(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
  retry: false,
})

export const syncSettingsQuery = queryOptions({
  queryKey: ['sync-settings'] as const,
  queryFn: () => api.syncSettings(),
  staleTime: Infinity,
  retry: false,
})
