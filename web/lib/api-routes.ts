/**
 * Centralised API route constants — single source of truth for endpoint paths.
 * Used by both application code and tests.
 */

export const API_ROUTES = {
  // Auth (public)
  AUTH_LOGIN: '/v1/auth/login',
  AUTH_LOGOUT: '/v1/auth/logout',

  // Setup (public)
  SETUP_STATUS: '/v1/setup/status',
  SETUP: '/v1/setup',

  // Keys
  KEYS: '/v1/keys',
  KEY: (id: string) => `/v1/keys/${id}`,
  KEY_REGENERATE: (id: string) => `/v1/keys/${id}/regenerate`,

  // Providers
  PROVIDERS: '/v1/providers',
  PROVIDER: (id: string) => `/v1/providers/${id}`,
  PROVIDER_HEALTHCHECK: (id: string) => `/v1/providers/${id}/healthcheck`,
  PROVIDER_MODELS: (id: string) => `/v1/providers/${id}/models`,
  PROVIDER_MODELS_SYNC: (id: string) => `/v1/providers/${id}/models/sync`,
  PROVIDER_KEY: (id: string) => `/v1/providers/${id}/key`,
  PROVIDER_SELECTED_MODELS: (id: string) => `/v1/providers/${id}/selected-models`,
  PROVIDER_SYNC: (id: string) => `/v1/providers/${id}/sync`,
  PROVIDERS_SYNC: '/v1/providers/sync',

  // Accounts
  ACCOUNTS: '/v1/accounts',
  ACCOUNT: (id: string) => `/v1/accounts/${id}`,
  ACCOUNT_ACTIVE: (id: string) => `/v1/accounts/${id}/active`,
  ACCOUNT_RESET_LINK: (id: string) => `/v1/accounts/${id}/reset-link`,
  ACCOUNT_SESSIONS: (id: string) => `/v1/accounts/${id}/sessions`,

  // Sessions
  SESSION: (id: string) => `/v1/sessions/${id}`,

  // Servers
  SERVERS: '/v1/servers',
  SERVER: (id: string) => `/v1/servers/${id}`,
  SERVER_METRICS: (id: string) => `/v1/servers/${id}/metrics`,
  SERVER_METRICS_HISTORY: (id: string) => `/v1/servers/${id}/metrics/history`,

  // Dashboard
  DASHBOARD_STATS: '/v1/dashboard/stats',
  DASHBOARD_OVERVIEW: '/v1/dashboard/overview',
  DASHBOARD_JOBS: '/v1/dashboard/jobs',
  DASHBOARD_JOB: (id: string) => `/v1/dashboard/jobs/${id}`,
  DASHBOARD_PERFORMANCE: '/v1/dashboard/performance',
  DASHBOARD_ANALYTICS: '/v1/dashboard/analytics',
  DASHBOARD_QUEUE_DEPTH: '/v1/dashboard/queue/depth',
  DASHBOARD_CAPACITY: '/v1/dashboard/capacity',
  DASHBOARD_CAPACITY_SETTINGS: '/v1/dashboard/capacity/settings',
  DASHBOARD_LAB: '/v1/dashboard/lab',
  DASHBOARD_SESSION_GROUPING: '/v1/dashboard/session-grouping/trigger',

  // Usage
  USAGE: '/v1/usage',
  USAGE_KEY: (keyId: string) => `/v1/usage/${keyId}`,
  USAGE_KEY_JOBS: (keyId: string) => `/v1/usage/${keyId}/jobs`,
  USAGE_KEY_MODELS: (keyId: string) => `/v1/usage/${keyId}/models`,
  USAGE_BREAKDOWN: '/v1/usage/breakdown',

  // Inference
  INFERENCE: '/v1/inference',
  INFERENCE_STREAM: (jobId: string) => `/v1/inference/${jobId}/stream`,
  INFERENCE_STATUS: (jobId: string) => `/v1/inference/${jobId}/status`,
  INFERENCE_CANCEL: (jobId: string) => `/v1/inference/${jobId}`,

  // OpenAI-compatible
  CHAT_COMPLETIONS: '/v1/chat/completions',

  // Gemini
  GEMINI_POLICIES: '/v1/gemini/policies',
  GEMINI_POLICY: (modelName: string) => `/v1/gemini/policies/${encodeURIComponent(modelName)}`,
  GEMINI_SYNC_CONFIG: '/v1/gemini/sync-config',
  GEMINI_MODELS_SYNC: '/v1/gemini/models/sync',
  GEMINI_SYNC_STATUS: '/v1/gemini/sync-status',
  GEMINI_MODELS: '/v1/gemini/models',

  // Ollama
  OLLAMA_MODELS: '/v1/ollama/models',
  OLLAMA_MODELS_SYNC: '/v1/ollama/models/sync',
  OLLAMA_SYNC_STATUS: '/v1/ollama/sync/status',
  OLLAMA_MODEL_PROVIDERS: (modelName: string) => `/v1/ollama/models/${encodeURIComponent(modelName)}/providers`,
  OLLAMA_PROVIDER_MODELS: (providerId: string) => `/v1/ollama/providers/${providerId}/models`,

  // Audit
  AUDIT: '/v1/audit',

  // System
  HEALTH: '/health',
  READYZ: '/readyz',
  METRICS_TARGETS: '/v1/metrics/targets',
} as const
