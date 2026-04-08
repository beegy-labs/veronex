import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, STALE_TIME_FAST, STALE_TIME_LIVE, REFETCH_INTERVAL_FAST, withJitter } from '@/lib/constants'

// ── LLM providers list ─────────────────────────────────────────────────────────

export const providersQuery = (params?: { search?: string; page?: number; limit?: number; provider_type?: string }) => queryOptions({
  queryKey: ['providers', params] as const,
  queryFn: () => api.providers(params),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
})


export const providerKeyQuery = (providerId: string) => queryOptions({
  queryKey: ['provider-key', providerId] as const,
  queryFn: () => api.providerKey(providerId),
  enabled: false,
})

export const ollamaModelProvidersQuery = (
  modelName: string,
  params?: { search?: string; page?: number; limit?: number },
) => queryOptions({
  queryKey: ['ollama-model-providers', modelName, params] as const,
  queryFn: () => api.ollamaModelProviders(modelName, params),
  staleTime: STALE_TIME_FAST,
})

export const selectedModelsQuery = (providerId: string) => queryOptions({
  queryKey: ['selected-models', providerId] as const,
  queryFn: () => api.getSelectedModels(providerId),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

// ── Ollama ────────────────────────────────────────────────────────────────────

export const ollamaModelsQuery = (
  params?: { search?: string; page?: number; limit?: number },
) => queryOptions({
  queryKey: ['ollama-models', params] as const,
  queryFn: () => api.ollamaModels(params),
  staleTime: STALE_TIME_SLOW,
  retry: false,
})

export const ollamaSyncStatusQuery = queryOptions({
  queryKey: ['ollama-sync-status'] as const,
  queryFn: () => api.ollamaSyncStatus(),
  staleTime: STALE_TIME_LIVE,
  retry: false,
})

export const globalModelSettingsQuery = queryOptions({
  queryKey: ['global-model-settings'] as const,
  queryFn: () => api.globalModelSettings(),
  staleTime: STALE_TIME_SLOW,
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

export const capacityQuery = (params?: { search?: string; page?: number; limit?: number }) => queryOptions({
  queryKey: ['capacity', params] as const,
  queryFn: () => api.capacity(params),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
  retry: false,
})

export const syncSettingsQuery = queryOptions({
  queryKey: ['sync-settings'] as const,
  queryFn: () => api.syncSettings(),
  staleTime: Infinity,
  retry: false,
})

export const capacityClusterQuery = queryOptions({
  queryKey: ['capacity-cluster'] as const,
  queryFn: () => api.capacityCluster(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
  retry: false,
})
