import type { AnalyticsStats, ApiKey, Backend, BackendSelectedModel, CreateKeyRequest, CreateKeyResponse, DashboardStats, GeminiModel, GeminiRateLimitPolicy, GeminiStatusSyncResponse, GeminiSyncConfig, GpuServer, HourlyUsage, Job, JobDetail, NodeMetrics, OllamaBackendForModel, OllamaModelWithCount, OllamaSyncJob, PerformanceStats, RegisterBackendRequest, RegisterBackendResponse, RegisterGpuServerRequest, ServerMetricsPoint, UpdateBackendRequest, UpdateGpuServerRequest, UpsertGeminiPolicyRequest, UsageAggregate, UsageBreakdown } from './types'

const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'
const KEY  = process.env.NEXT_PUBLIC_VERONEX_ADMIN_KEY ?? ''

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      'X-API-Key': KEY,
      'Content-Type': 'application/json',
      ...init?.headers,
    },
    cache: 'no-store',
  })
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

export const api = {
  stats: () =>
    req<DashboardStats>('/v1/dashboard/stats'),

  jobs: (params?: string) =>
    req<{ jobs: Job[]; total: number }>(
      `/v1/dashboard/jobs${params ? '?' + params : ''}`,
    ),

  jobDetail: (id: string) =>
    req<JobDetail>(`/v1/dashboard/jobs/${id}`),

  keys: () =>
    req<ApiKey[]>('/v1/keys'),

  createKey: (body: CreateKeyRequest) =>
    req<CreateKeyResponse>('/v1/keys', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteKey: (id: string) =>
    req<void>(`/v1/keys/${id}`, { method: 'DELETE' }),

  toggleKeyActive: (id: string, is_active: boolean) =>
    req<void>(`/v1/keys/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ is_active }),
    }),

  submitInference: (body: { prompt: string; model: string; backend: string }) =>
    req<{ job_id: string }>('/v1/inference', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  cancelJob: (id: string) =>
    req<void>(`/v1/inference/${id}`, { method: 'DELETE' }),

  servers: () =>
    req<GpuServer[]>('/v1/servers'),

  registerServer: (body: RegisterGpuServerRequest) =>
    req<{ id: string }>('/v1/servers', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  updateServer: (id: string, body: UpdateGpuServerRequest) =>
    req<GpuServer>(`/v1/servers/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),

  deleteServer: (id: string) =>
    req<void>(`/v1/servers/${id}`, { method: 'DELETE' }),

  serverMetrics: (id: string) =>
    req<NodeMetrics>(`/v1/servers/${id}/metrics`),

  serverMetricsHistory: (id: string, hours = 1) =>
    req<ServerMetricsPoint[]>(`/v1/servers/${id}/metrics/history?hours=${hours}`),

  backends: () =>
    req<Backend[]>('/v1/backends'),

  registerBackend: (body: RegisterBackendRequest) =>
    req<RegisterBackendResponse>('/v1/backends', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteBackend: (id: string) =>
    req<void>(`/v1/backends/${id}`, { method: 'DELETE' }),

  updateBackend: (id: string, body: UpdateBackendRequest) =>
    req<Backend>(`/v1/backends/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),

  healthcheckBackend: (id: string) =>
    req<{ id: string; status: string }>(`/v1/backends/${id}/healthcheck`, { method: 'POST' }),

  backendModels: (id: string) =>
    req<{ models: string[] }>(`/v1/backends/${id}/models`),

  syncBackendModels: (id: string) =>
    req<{ models: string[]; synced: boolean }>(`/v1/backends/${id}/models/sync`, { method: 'POST' }),

  backendKey: (id: string) =>
    req<{ key: string }>(`/v1/backends/${id}/key`),

  getSelectedModels: (backendId: string) =>
    req<{ models: BackendSelectedModel[] }>(`/v1/backends/${backendId}/selected-models`),

  setModelEnabled: (backendId: string, modelName: string, isEnabled: boolean) =>
    req<void>(`/v1/backends/${backendId}/selected-models/${encodeURIComponent(modelName)}`, {
      method: 'PATCH',
      body: JSON.stringify({ is_enabled: isEnabled }),
    }),

  usageAggregate: (hours = 24) =>
    req<UsageAggregate>(`/v1/usage?hours=${hours}`),

  keyUsage: (keyId: string, hours = 24) =>
    req<HourlyUsage[]>(`/v1/usage/${keyId}?hours=${hours}`),

  performance: (hours = 24) =>
    req<PerformanceStats>(`/v1/dashboard/performance?hours=${hours}`),

  analytics: (hours = 24) =>
    req<AnalyticsStats>(`/v1/dashboard/analytics?hours=${hours}`),

  usageBreakdown: (hours = 24) =>
    req<UsageBreakdown>(`/v1/usage/breakdown?hours=${hours}`),

  geminiPolicies: () =>
    req<GeminiRateLimitPolicy[]>('/v1/gemini/policies'),

  upsertGeminiPolicy: (modelName: string, body: UpsertGeminiPolicyRequest) =>
    req<GeminiRateLimitPolicy>(`/v1/gemini/policies/${encodeURIComponent(modelName)}`, {
      method: 'PUT',
      body: JSON.stringify(body),
    }),

  geminiSyncConfig: () =>
    req<GeminiSyncConfig>('/v1/gemini/sync-config'),

  setGeminiSyncConfig: (api_key: string) =>
    req<void>('/v1/gemini/sync-config', {
      method: 'PUT',
      body: JSON.stringify({ api_key }),
    }),

  syncGeminiModels: () =>
    req<{ models: string[]; count: number }>('/v1/gemini/models/sync', { method: 'POST' }),

  syncGeminiStatus: () =>
    req<GeminiStatusSyncResponse>('/v1/gemini/sync-status', { method: 'POST' }),

  geminiModels: () =>
    req<{ models: GeminiModel[] }>('/v1/gemini/models'),

  ollamaModels: () =>
    req<{ models: OllamaModelWithCount[] }>('/v1/ollama/models'),

  syncOllamaModels: () =>
    req<{ job_id: string; status: string }>('/v1/ollama/models/sync', { method: 'POST' }),

  ollamaSyncStatus: () =>
    req<OllamaSyncJob>('/v1/ollama/sync/status'),

  ollamaModelBackends: (modelName: string) =>
    req<{ backends: OllamaBackendForModel[] }>(`/v1/ollama/models/${encodeURIComponent(modelName)}/backends`),

  ollamaBackendModels: (backendId: string) =>
    req<{ models: string[] }>(`/v1/ollama/backends/${backendId}/models`),
}
