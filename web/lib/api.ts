import type { ApiKey, Backend, CreateKeyRequest, CreateKeyResponse, DashboardStats, GpuServer, HourlyUsage, Job, NodeMetrics, PerformanceStats, RegisterBackendRequest, RegisterBackendResponse, RegisterGpuServerRequest, ServerMetricsPoint, UpdateBackendRequest, UsageAggregate } from './types'

const BASE = process.env.NEXT_PUBLIC_INFERQ_API_URL ?? 'http://localhost:3001'
const KEY  = process.env.NEXT_PUBLIC_INFERQ_ADMIN_KEY ?? ''

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

  keys: () =>
    req<ApiKey[]>('/v1/keys'),

  createKey: (body: CreateKeyRequest) =>
    req<CreateKeyResponse>('/v1/keys', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  revokeKey: (id: string) =>
    req<void>(`/v1/keys/${id}`, { method: 'DELETE' }),

  submitInference: (body: { prompt: string; model: string; backend: string }) =>
    req<{ job_id: string }>('/v1/inference', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  servers: () =>
    req<GpuServer[]>('/v1/servers'),

  registerServer: (body: RegisterGpuServerRequest) =>
    req<{ id: string }>('/v1/servers', {
      method: 'POST',
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

  usageAggregate: (hours = 24) =>
    req<UsageAggregate>(`/v1/usage?hours=${hours}`),

  keyUsage: (keyId: string, hours = 24) =>
    req<HourlyUsage[]>(`/v1/usage/${keyId}?hours=${hours}`),

  performance: (hours = 24) =>
    req<PerformanceStats>(`/v1/dashboard/performance?hours=${hours}`),
}
