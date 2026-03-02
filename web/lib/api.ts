import type { Account, AnalyticsStats, ApiKey, AuditEvent, Backend, BackendSelectedModel, CapacityResponse, CapacitySettings, CreateAccountRequest, CreateAccountResponse, CreateKeyRequest, CreateKeyResponse, DashboardStats, GeminiModel, GeminiRateLimitPolicy, GeminiStatusSyncResponse, GeminiSyncConfig, GpuServer, HourlyUsage, Job, JobDetail, LabSettings, LoginRequest, LoginResponse, ModelBreakdown, NodeMetrics, OllamaBackendForModel, OllamaModelWithCount, OllamaSyncJob, PatchCapacitySettings, PatchLabSettings, PerformanceStats, QueueDepth, RegisterBackendRequest, RegisterBackendResponse, RegisterGpuServerRequest, ServerMetricsPoint, SessionRecord, UpdateBackendRequest, UpdateGpuServerRequest, UpsertGeminiPolicyRequest, UsageAggregate, UsageBreakdown } from './types'
import { apiClient } from './api-client'

const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

/** Plain fetch for public routes (setup, auth) — no auth header. */
async function fetchPublic<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    cache: 'no-store',
  })
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

export const api = {
  // ── Dashboard (JWT-protected) ─────────────────────────────────────────────
  stats: () =>
    apiClient.get<DashboardStats>('/v1/dashboard/stats'),

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

  capacitySettings: () =>
    apiClient.get<CapacitySettings>('/v1/dashboard/capacity/settings'),

  patchCapacitySettings: (body: PatchCapacitySettings) =>
    apiClient.patch<CapacitySettings>('/v1/dashboard/capacity/settings', body),

  triggerCapacitySync: () =>
    apiClient.post<{ message: string }>('/v1/dashboard/capacity/sync', {}),

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

  updateServer: (id: string, body: UpdateGpuServerRequest) =>
    apiClient.patch<GpuServer>(`/v1/servers/${id}`, body),

  deleteServer: (id: string) =>
    apiClient.delete<void>(`/v1/servers/${id}`),

  serverMetrics: (id: string) =>
    apiClient.get<NodeMetrics>(`/v1/servers/${id}/metrics`),

  serverMetricsHistory: (id: string, hours = 1) =>
    apiClient.get<ServerMetricsPoint[]>(`/v1/servers/${id}/metrics/history?hours=${hours}`),

  // ── Backends (JWT-protected) ──────────────────────────────────────────────
  backends: () =>
    apiClient.get<Backend[]>('/v1/backends'),

  registerBackend: (body: RegisterBackendRequest) =>
    apiClient.post<RegisterBackendResponse>('/v1/backends', body),

  deleteBackend: (id: string) =>
    apiClient.delete<void>(`/v1/backends/${id}`),

  updateBackend: (id: string, body: UpdateBackendRequest) =>
    apiClient.patch<Backend>(`/v1/backends/${id}`, body),

  healthcheckBackend: (id: string) =>
    apiClient.post<{ id: string; status: string }>(`/v1/backends/${id}/healthcheck`),

  backendModels: (id: string) =>
    apiClient.get<{ models: string[] }>(`/v1/backends/${id}/models`),

  syncBackendModels: (id: string) =>
    apiClient.post<{ models: string[]; synced: boolean }>(`/v1/backends/${id}/models/sync`),

  backendKey: (id: string) =>
    apiClient.get<{ key: string }>(`/v1/backends/${id}/key`),

  getSelectedModels: (backendId: string) =>
    apiClient.get<{ models: BackendSelectedModel[] }>(`/v1/backends/${backendId}/selected-models`),

  setModelEnabled: (backendId: string, modelName: string, isEnabled: boolean) =>
    apiClient.patch<void>(`/v1/backends/${backendId}/selected-models/${encodeURIComponent(modelName)}`, { is_enabled: isEnabled }),

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

  ollamaModelBackends: (modelName: string) =>
    apiClient.get<{ backends: OllamaBackendForModel[] }>(`/v1/ollama/models/${encodeURIComponent(modelName)}/backends`),

  ollamaBackendModels: (backendId: string) =>
    apiClient.get<{ models: string[] }>(`/v1/ollama/backends/${backendId}/models`),

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

  logout: (refresh_token: string) =>
    fetchPublic<void>('/v1/auth/logout', {
      method: 'POST',
      body: JSON.stringify({ refresh_token }),
    }),

  // ── Accounts (JWT-protected) ──────────────────────────────────────────────
  accounts: () =>
    apiClient.get<Account[]>('/v1/accounts'),

  createAccount: (body: CreateAccountRequest) =>
    apiClient.post<CreateAccountResponse>('/v1/accounts', body),

  updateAccount: (id: string, body: Partial<Pick<Account, 'name' | 'email' | 'department' | 'position'>>) =>
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

  // ── Audit (JWT-protected) ─────────────────────────────────────────────────
  auditEvents: (params?: { limit?: number; offset?: number; action?: string; resource_type?: string }) => {
    const qs = new URLSearchParams()
    if (params?.limit) qs.set('limit', String(params.limit))
    if (params?.offset) qs.set('offset', String(params.offset))
    if (params?.action) qs.set('action', params.action)
    if (params?.resource_type) qs.set('resource_type', params.resource_type)
    const q = qs.toString()
    return apiClient.get<AuditEvent[]>(`/v1/audit${q ? '?' + q : ''}`)
  },
}
