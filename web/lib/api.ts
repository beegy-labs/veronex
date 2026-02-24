import type { ApiKey, Backend, CreateKeyRequest, CreateKeyResponse, DashboardStats, HourlyUsage, Job, PerformanceStats, RegisterBackendRequest, RegisterBackendResponse, UsageAggregate } from './types'

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

  backends: () =>
    req<Backend[]>('/v1/backends'),

  registerBackend: (body: RegisterBackendRequest) =>
    req<RegisterBackendResponse>('/v1/backends', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteBackend: (id: string) =>
    req<void>(`/v1/backends/${id}`, { method: 'DELETE' }),

  healthcheckBackend: (id: string) =>
    req<{ id: string; status: string }>(`/v1/backends/${id}/healthcheck`, { method: 'POST' }),

  backendModels: (id: string) =>
    req<{ models: string[] }>(`/v1/backends/${id}/models`),

  usageAggregate: (hours = 24) =>
    req<UsageAggregate>(`/v1/usage?hours=${hours}`),

  keyUsage: (keyId: string, hours = 24) =>
    req<HourlyUsage[]>(`/v1/usage/${keyId}?hours=${hours}`),

  performance: (hours = 24) =>
    req<PerformanceStats>(`/v1/dashboard/performance?hours=${hours}`),
}
