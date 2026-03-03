import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── LLM providers list ─────────────────────────────────────────────────────────

export const providersQuery = queryOptions({
  queryKey: ['providers'] as const,
  queryFn: () => api.providers(),
  staleTime: 29_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
})

// ── Provider-specific ──────────────────────────────────────────────────────────

export const providerModelsQuery = (providerId: string) => queryOptions({
  queryKey: ['provider-models', backendId] as const,
  queryFn: () => api.providerModels(providerId),
  staleTime: 59_000,
  retry: false,
})

export const selectedModelsQuery = (providerId: string) => queryOptions({
  queryKey: ['selected-models', backendId] as const,
  queryFn: () => api.getSelectedModels(providerId),
  staleTime: 59_000,
  retry: false,
})

// ── Ollama ────────────────────────────────────────────────────────────────────

export const ollamaModelsQuery = queryOptions({
  queryKey: ['ollama-models'] as const,
  queryFn: () => api.ollamaModels(),
  staleTime: 59_000,
  retry: false,
})

export const ollamaSyncStatusQuery = queryOptions({
  queryKey: ['ollama-sync-status'] as const,
  queryFn: () => api.ollamaSyncStatus(),
  staleTime: 4_900,
  retry: false,
})

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
  staleTime: 59_000,
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
  staleTime: 29_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
  retry: false,
})

export const capacitySettingsQuery = queryOptions({
  queryKey: ['capacity-settings'] as const,
  queryFn: () => api.capacitySettings(),
  staleTime: Infinity,
  retry: false,
})
