import type { Account, AnalyticsStats, ApiKey, AuditEvent, Provider, ProviderSelectedModel, CapacityResponse, RoleSummary, SyncSettings, CreateAccountRequest, CreateAccountResponse, CreateKeyRequest, CreateKeyResponse, DashboardStats, GeminiModel, GeminiRateLimitPolicy, GeminiStatusSyncResponse, GeminiSyncConfig, GpuServer, HourlyUsage, Job, JobDetail, LabSettings, LoginRequest, LoginResponse, ModelBreakdown, NodeMetrics, OllamaProviderForModel, OllamaModelWithCount, OllamaSyncJob, PatchSyncSettings, PatchLabSettings, PerformanceStats, QueueDepth, RegisterProviderRequest, RegisterProviderResponse, RegisterGpuServerRequest, ServerMetricsPoint, SessionRecord, UpdateProviderRequest, UpdateGpuServerRequest, UpsertGeminiPolicyRequest, UsageAggregate, UsageBreakdown } from './types'
import { ApiHttpError } from './types'
import { apiClient } from './api-client'
import { BASE_API_URL } from './constants'

export const BASE = BASE_API_URL

/** Plain fetch for public routes (setup, auth) — credentials included for HttpOnly cookies. */
async function fetchPublic<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    credentials: 'include',
    cache: 'no-store',
  })
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

/** Shared verify fetch — POST url to verify endpoint, handle network errors. */
async function verifyEndpoint(path: string, url: string): Promise<{ reachable: boolean }> {
  let res: Response
  try {
    res = await fetch(`${BASE}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ url }),
    })
  } catch {
    throw new ApiHttpError('NETWORK_ERROR', 0)
  }
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new ApiHttpError((data as { error?: string }).error ?? `${res.status}`, res.status)
  return data as { reachable: boolean }
}

export const api = {
  // ── Dashboard (JWT-protected) ─────────────────────────────────────────────
  stats: () =>
    apiClient.get<DashboardStats>('/v1/dashboard/stats'),

  overview: () =>
    apiClient.get<import('./types').DashboardOverview>('/v1/dashboard/overview'),

  jobs: (params?: string) =>
    apiClient.get<{ jobs: Job[]; total: number }>(
      `/v1/dashboard/jobs${params ? '?' + params : ''}`,
    ),

  jobDetail: (id: string) =>
    apiClient.get<JobDetail>(`/v1/dashboard/jobs/${id}`),

  cancelJob: (id: string) =>
    apiClient.delete<void>(`/v1/dashboard/jobs/${id}`),

  performance: (hours = 24) =>
    apiClient.get<PerformanceStats>(`/v1/dashboard/performance?hours=${hours}`),

  analytics: (hours = 24) =>
    apiClient.get<AnalyticsStats>(`/v1/dashboard/analytics?hours=${hours}`),

  // ── Queue depth (JWT-protected) ──────────────────────────────────────────
  queueDepth: () =>
    apiClient.get<QueueDepth>('/v1/dashboard/queue/depth'),

  // ── Capacity (JWT-protected) ──────────────────────────────────────────────
  capacity: () =>
    apiClient.get<CapacityResponse>('/v1/dashboard/capacity'),

  syncSettings: () =>
    apiClient.get<SyncSettings>('/v1/dashboard/capacity/settings'),

  patchSyncSettings: (body: PatchSyncSettings) =>
    apiClient.patch<SyncSettings>('/v1/dashboard/capacity/settings', body),

  triggerSessionGrouping: (beforeDate?: string) =>
    apiClient.post<{ message: string }>('/v1/dashboard/session-grouping/trigger', {
      before_date: beforeDate ?? null,
    }),

  // ── Lab (experimental) features (JWT-protected) ───────────────────────────
  labSettings: () =>
    apiClient.get<LabSettings>('/v1/dashboard/lab'),

  patchLabSettings: (body: PatchLabSettings) =>
    apiClient.patch<LabSettings>('/v1/dashboard/lab', body),

  // ── Key management (JWT-protected) ────────────────────────────────────────
  keys: () =>
    apiClient.get<ApiKey[]>('/v1/keys'),

  createKey: (body: CreateKeyRequest) =>
    apiClient.post<CreateKeyResponse>('/v1/keys', body),

  deleteKey: (id: string) =>
    apiClient.delete<void>(`/v1/keys/${id}`),

  toggleKeyActive: (id: string, is_active: boolean) =>
    apiClient.patch<void>(`/v1/keys/${id}`, { is_active }),

  updateKeyTier: (id: string, tier: 'free' | 'paid') =>
    apiClient.patch<void>(`/v1/keys/${id}`, { tier }),

  regenerateKey: (id: string) =>
    apiClient.post<CreateKeyResponse>(`/v1/keys/${id}/regenerate`),

  // ── Usage (JWT-protected) ─────────────────────────────────────────────────
  usageAggregate: (hours = 24) =>
    apiClient.get<UsageAggregate>(`/v1/usage?hours=${hours}`),

  keyModelBreakdown: (keyId: string, hours = 24) =>
    apiClient.get<ModelBreakdown[]>(`/v1/usage/${keyId}/models?hours=${hours}`),

  keyUsage: (keyId: string, hours = 24) =>
    apiClient.get<HourlyUsage[]>(`/v1/usage/${keyId}?hours=${hours}`),

  usageBreakdown: (hours = 24) =>
    apiClient.get<UsageBreakdown>(`/v1/usage/breakdown?hours=${hours}`),

  // ── GPU servers (JWT-protected) ───────────────────────────────────────────
  servers: () =>
    apiClient.get<GpuServer[]>('/v1/servers'),

  registerServer: (body: RegisterGpuServerRequest) =>
    apiClient.post<{ id: string }>('/v1/servers', body),

  verifyServer: (url: string) => verifyEndpoint('/v1/servers/verify', url),

  updateServer: (id: string, body: UpdateGpuServerRequest) =>
    apiClient.patch<GpuServer>(`/v1/servers/${id}`, body),

  deleteServer: (id: string) =>
    apiClient.delete<void>(`/v1/servers/${id}`),

  serverMetrics: (id: string) =>
    apiClient.get<NodeMetrics>(`/v1/servers/${id}/metrics`),

  serverMetricsHistory: (id: string, hours = 1) =>
    apiClient.get<ServerMetricsPoint[]>(`/v1/servers/${id}/metrics/history?hours=${hours}`),

  // ── Providers (JWT-protected) ──────────────────────────────────────────────
  providers: () =>
    apiClient.get<Provider[]>('/v1/providers'),

  registerProvider: (body: RegisterProviderRequest) =>
    apiClient.post<RegisterProviderResponse>('/v1/providers', body),

  verifyProvider: (url: string) => verifyEndpoint('/v1/providers/verify', url),

  deleteProvider: (id: string) =>
    apiClient.delete<void>(`/v1/providers/${id}`),

  updateProvider: (id: string, body: UpdateProviderRequest) =>
    apiClient.patch<Provider>(`/v1/providers/${id}`, body),

  syncProvider: (id: string) =>
    apiClient.post<{ message: string }>(`/v1/providers/${id}/sync`),

  syncAllProviders: () =>
    apiClient.post<{ message: string }>('/v1/providers/sync'),

  providerModels: (id: string) =>
    apiClient.get<{ models: string[] }>(`/v1/providers/${id}/models`),

  providerKey: (id: string) =>
    apiClient.get<{ key: string }>(`/v1/providers/${id}/key`),

  getSelectedModels: (providerId: string) =>
    apiClient.get<{ models: ProviderSelectedModel[] }>(`/v1/providers/${providerId}/selected-models`),

  setModelEnabled: (providerId: string, modelName: string, isEnabled: boolean) =>
    apiClient.patch<void>(`/v1/providers/${providerId}/selected-models/${encodeURIComponent(modelName)}`, { is_enabled: isEnabled }),

  // ── Gemini (JWT-protected) ────────────────────────────────────────────────
  geminiPolicies: () =>
    apiClient.get<GeminiRateLimitPolicy[]>('/v1/gemini/policies'),

  upsertGeminiPolicy: (modelName: string, body: UpsertGeminiPolicyRequest) =>
    apiClient.put<GeminiRateLimitPolicy>(`/v1/gemini/policies/${encodeURIComponent(modelName)}`, body),

  geminiSyncConfig: () =>
    apiClient.get<GeminiSyncConfig>('/v1/gemini/sync-config'),

  setGeminiSyncConfig: (api_key: string) =>
    apiClient.put<void>('/v1/gemini/sync-config', { api_key }),

  syncGeminiModels: () =>
    apiClient.post<{ models: string[]; count: number }>('/v1/gemini/models/sync'),

  syncGeminiStatus: () =>
    apiClient.post<GeminiStatusSyncResponse>('/v1/gemini/sync-status'),

  geminiModels: () =>
    apiClient.get<{ models: GeminiModel[] }>('/v1/gemini/models'),

  // ── Ollama (JWT-protected) ────────────────────────────────────────────────
  ollamaModels: () =>
    apiClient.get<{ models: OllamaModelWithCount[] }>('/v1/ollama/models'),

  syncOllamaModels: () =>
    apiClient.post<{ job_id: string; status: string }>('/v1/ollama/models/sync'),

  ollamaSyncStatus: () =>
    apiClient.get<OllamaSyncJob>('/v1/ollama/sync/status'),

  ollamaModelProviders: (modelName: string) =>
    apiClient.get<{ providers: OllamaProviderForModel[] }>(`/v1/ollama/models/${encodeURIComponent(modelName)}/providers`),

  ollamaProviderModels: (providerId: string) =>
    apiClient.get<{ models: string[] }>(`/v1/ollama/providers/${providerId}/models`),

  // ── Setup (public — no auth, first-run only) ──────────────────────────────
  setupStatus: () =>
    fetchPublic<{ needs_setup: boolean }>('/v1/setup/status'),

  setup: (body: { username: string; password: string }) =>
    fetchPublic<LoginResponse>('/v1/setup', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  // ── Auth (public) ─────────────────────────────────────────────────────────
  login: (body: LoginRequest) =>
    fetchPublic<LoginResponse>('/v1/auth/login', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  logout: () =>
    fetchPublic<void>('/v1/auth/logout', {
      method: 'POST',
    }),

  // ── Accounts (JWT-protected) ──────────────────────────────────────────────
  accounts: () =>
    apiClient.get<Account[]>('/v1/accounts'),

  createAccount: (body: CreateAccountRequest) =>
    apiClient.post<CreateAccountResponse>('/v1/accounts', body),

  updateAccount: (id: string, body: Partial<Pick<Account, 'name' | 'email' | 'department' | 'position'>> & { role_ids?: string[] }) =>
    apiClient.patch<void>(`/v1/accounts/${id}`, body),

  deleteAccount: (id: string) =>
    apiClient.delete<void>(`/v1/accounts/${id}`),

  setAccountActive: (id: string, is_active: boolean) =>
    apiClient.patch<void>(`/v1/accounts/${id}/active`, { is_active }),

  createResetLink: (id: string) =>
    apiClient.post<{ token: string }>(`/v1/accounts/${id}/reset-link`),

  accountSessions: (id: string) =>
    apiClient.get<SessionRecord[]>(`/v1/accounts/${id}/sessions`),

  revokeSession: (sessionId: string) =>
    apiClient.delete<void>(`/v1/sessions/${sessionId}`),

  revokeAllSessions: (accountId: string) =>
    apiClient.delete<void>(`/v1/accounts/${accountId}/sessions`),

  // ── Roles (JWT-protected, super only) ────────────────────────────────────
  roles: () =>
    apiClient.get<RoleSummary[]>('/v1/roles'),

  createRole: (body: { name: string; permissions: string[]; menus: string[] }) =>
    apiClient.post<RoleSummary>('/v1/roles', body),

  updateRole: (id: string, body: { name?: string; permissions?: string[]; menus?: string[] }) =>
    apiClient.patch<void>(`/v1/roles/${id}`, body),

  deleteRole: (id: string) =>
    apiClient.delete<void>(`/v1/roles/${id}`),

  // ── Audit (JWT-protected) ─────────────────────────────────────────────────
  auditEvents: (params?: { limit?: number; offset?: number; action?: string; resource_type?: string; resource_id?: string }) => {
    const qs = new URLSearchParams()
    if (params?.limit != null) qs.set('limit', String(params.limit))
    if (params?.offset != null) qs.set('offset', String(params.offset))
    if (params?.action) qs.set('action', params.action)
    if (params?.resource_type) qs.set('resource_type', params.resource_type)
    if (params?.resource_id) qs.set('resource_id', params.resource_id)
    const q = qs.toString()
    return apiClient.get<AuditEvent[]>(`/v1/audit${q ? '?' + q : ''}`)
  },
}

// ── Verify error message helper ──────────────────────────────────────────────

interface VerifyErrorLabels {
  duplicate: string; network: string; unreachable: string; fallback: string
}

export function verifyErrorMessage(e: unknown, labels: VerifyErrorLabels): string {
  if (e instanceof ApiHttpError) {
    if (e.status === 409) return labels.duplicate
    if (e.message === 'NETWORK_ERROR') return labels.network
    if (e.status === 502) return labels.unreachable
    return e.message
  }
  if (e instanceof Error) return e.message
  return labels.fallback
}
