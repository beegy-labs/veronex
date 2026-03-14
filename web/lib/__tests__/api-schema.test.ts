/**
 * API schema validation tests.
 *
 * Validates that Zod schemas defined in api-schemas.ts are internally
 * consistent and accept well-formed payloads matching the OpenAPI spec
 * and TypeScript types in lib/types.ts.
 *
 * These are pure unit tests — no HTTP calls. They guard against
 * accidental schema drift between the frontend types and backend API.
 */

import { describe, it, expect } from 'vitest'
import {
  ApiKeySchema,
  ApiKeyListSchema,
  CreateKeyResponseSchema,
  ProviderSchema,
  ProviderListSchema,
  RegisterProviderResponseSchema,
  AccountSchema,
  AccountListSchema,
  CreateAccountResponseSchema,
  GpuServerSchema,
  GpuServerListSchema,
  DashboardStatsSchema,
  QueueDepthSchema,
  JobSchema,
  PaginatedJobsSchema,
  JobDetailSchema,
  PerformanceStatsSchema,
  HourlyThroughputSchema,
  AnalyticsStatsSchema,
  LabSettingsSchema,
  ApiErrorSchema,
} from '../api-schemas'

// ── Fixtures ────────────────────────────────────────────────────────────────

const fixtures = {
  apiKey: {
    id: '550e8400-e29b-41d4-a716-446655440000',
    key_prefix: 'sk-abc',
    name: 'test-key',
    tenant_id: 'default',
    is_active: true,
    rate_limit_rpm: 60,
    rate_limit_tpm: 10000,
    created_at: '2026-01-15T10:00:00Z',
    expires_at: null,
    tier: 'paid' as const,
  },

  createKeyResponse: {
    id: '550e8400-e29b-41d4-a716-446655440000',
    key: 'sk-abc123def456',
    key_prefix: 'sk-abc',
    tenant_id: 'default',
    created_at: '2026-01-15T10:00:00Z',
  },

  provider: {
    id: '660e8400-e29b-41d4-a716-446655440001',
    name: 'local-ollama',
    provider_type: 'ollama' as const,
    url: 'http://192.168.1.10:11434',
    is_active: true,
    total_vram_mb: 24576,
    gpu_index: 0,
    server_id: null,
    is_free_tier: false,
    status: 'online' as const,
    registered_at: '2026-01-15T10:00:00Z',
    api_key_masked: null,
  },

  registerProviderResponse: {
    id: '660e8400-e29b-41d4-a716-446655440001',
    status: 'offline',
  },

  account: {
    id: '770e8400-e29b-41d4-a716-446655440002',
    username: 'admin',
    name: 'Admin',
    email: null,
    role: 'admin' as const,
    department: null,
    position: null,
    is_active: true,
    last_login_at: '2026-01-15T10:00:00Z',
    created_at: '2026-01-01T00:00:00Z',
  },

  createAccountResponse: {
    id: '770e8400-e29b-41d4-a716-446655440002',
    username: 'new-user',
    role: 'admin',
    test_api_key: 'sk-test-xyz',
    created_at: '2026-01-15T10:00:00Z',
  },

  gpuServer: {
    id: '880e8400-e29b-41d4-a716-446655440003',
    name: 'gpu-node-01',
    node_exporter_url: 'http://192.168.1.10:9100',
    registered_at: '2026-01-15T10:00:00Z',
  },

  dashboardStats: {
    total_keys: 5,
    active_keys: 3,
    total_jobs: 1250,
    jobs_last_24h: 42,
    jobs_by_status: { completed: 1200, failed: 30, pending: 10, running: 5, cancelled: 5 },
  },

  queueDepth: {
    api_paid: 2,
    api: 5,
    test: 1,
    total: 8,
  },

  job: {
    id: '990e8400-e29b-41d4-a716-446655440004',
    model_name: 'llama3.2:3b',
    provider_type: 'ollama',
    status: 'completed' as const,
    source: 'api' as const,
    created_at: '2026-01-15T10:00:00Z',
    completed_at: '2026-01-15T10:00:05Z',
    latency_ms: 5000,
    ttft_ms: 120,
    prompt_tokens: 50,
    completion_tokens: 200,
    cached_tokens: null,
    tps: 40.0,
    api_key_name: 'test-key',
    account_name: null,
    request_path: '/v1/chat/completions',
    estimated_cost_usd: null,
    has_tool_calls: false,
  },

  jobDetail: {
    id: '990e8400-e29b-41d4-a716-446655440004',
    model_name: 'llama3.2:3b',
    provider_type: 'ollama',
    status: 'completed' as const,
    source: 'api' as const,
    created_at: '2026-01-15T10:00:00Z',
    completed_at: '2026-01-15T10:00:05Z',
    started_at: '2026-01-15T10:00:00Z',
    latency_ms: 5000,
    ttft_ms: 120,
    prompt_tokens: 50,
    completion_tokens: 200,
    cached_tokens: null,
    tps: 40.0,
    api_key_name: 'test-key',
    account_name: null,
    request_path: '/v1/chat/completions',
    estimated_cost_usd: null,
    prompt: 'Explain hexagonal architecture.',
    result_text: 'Hexagonal architecture...',
    error: null,
    tool_calls_json: null,
    message_count: 2,
    messages_json: null,
  },

  performanceStats: {
    avg_latency_ms: 1500,
    p50_latency_ms: 1200,
    p95_latency_ms: 3500,
    p99_latency_ms: 5000,
    total_requests: 100,
    success_rate: 95.5,
    total_tokens: 25000,
    hourly: [
      { hour: '2026-01-15T10:00:00Z', request_count: 10, success_count: 9, avg_latency_ms: 1400, total_tokens: 2500 },
    ],
  },

  analyticsStats: {
    avg_tps: 35.2,
    avg_prompt_tokens: 48,
    avg_completion_tokens: 180,
    models: [
      { model_name: 'llama3.2:3b', request_count: 80, success_count: 76, success_rate: 95.0, total_prompt_tokens: 3840, total_completion_tokens: 14400, avg_latency_ms: 1300 },
    ],
    finish_reasons: [
      { reason: 'stop', count: 76 },
      { reason: 'error', count: 4 },
    ],
  },

  labSettings: {
    gemini_function_calling: true,
    updated_at: '2026-01-15T10:00:00Z',
  },
}

// ── Tests ───────────────────────────────────────────────────────────────────

describe('API Schema: Keys', () => {
  it('validates a single API key', () => {
    expect(ApiKeySchema.safeParse(fixtures.apiKey).success).toBe(true)
  })

  it('validates API key list', () => {
    expect(ApiKeyListSchema.safeParse([fixtures.apiKey]).success).toBe(true)
  })

  it('validates empty API key list', () => {
    expect(ApiKeyListSchema.safeParse([]).success).toBe(true)
  })

  it('validates create key response', () => {
    expect(CreateKeyResponseSchema.safeParse(fixtures.createKeyResponse).success).toBe(true)
  })

  it('rejects key missing required fields', () => {
    const { id: _, ...noId } = fixtures.apiKey
    expect(ApiKeySchema.safeParse(noId).success).toBe(false)
  })

  it('rejects key with invalid tier', () => {
    expect(ApiKeySchema.safeParse({ ...fixtures.apiKey, tier: 'enterprise' }).success).toBe(false)
  })
})

describe('API Schema: Providers', () => {
  it('validates a single provider', () => {
    expect(ProviderSchema.safeParse(fixtures.provider).success).toBe(true)
  })

  it('validates provider list', () => {
    expect(ProviderListSchema.safeParse([fixtures.provider]).success).toBe(true)
  })

  it('validates register provider response', () => {
    expect(RegisterProviderResponseSchema.safeParse(fixtures.registerProviderResponse).success).toBe(true)
  })

  it('rejects provider with invalid provider_type', () => {
    expect(ProviderSchema.safeParse({ ...fixtures.provider, provider_type: 'openai' }).success).toBe(false)
  })

  it('rejects provider with invalid status', () => {
    expect(ProviderSchema.safeParse({ ...fixtures.provider, status: 'unknown' }).success).toBe(false)
  })
})

describe('API Schema: Accounts', () => {
  it('validates a single account', () => {
    expect(AccountSchema.safeParse(fixtures.account).success).toBe(true)
  })

  it('validates account list', () => {
    expect(AccountListSchema.safeParse([fixtures.account]).success).toBe(true)
  })

  it('validates create account response', () => {
    expect(CreateAccountResponseSchema.safeParse(fixtures.createAccountResponse).success).toBe(true)
  })

  it('rejects account with invalid role', () => {
    expect(AccountSchema.safeParse({ ...fixtures.account, role: 'viewer' }).success).toBe(false)
  })
})

describe('API Schema: Servers', () => {
  it('validates a single GPU server', () => {
    expect(GpuServerSchema.safeParse(fixtures.gpuServer).success).toBe(true)
  })

  it('validates GPU server list', () => {
    expect(GpuServerListSchema.safeParse([fixtures.gpuServer]).success).toBe(true)
  })

  it('validates empty server list', () => {
    expect(GpuServerListSchema.safeParse([]).success).toBe(true)
  })
})

describe('API Schema: Dashboard Stats', () => {
  it('validates dashboard stats', () => {
    expect(DashboardStatsSchema.safeParse(fixtures.dashboardStats).success).toBe(true)
  })

  it('rejects stats with missing total_jobs', () => {
    const { total_jobs: _, ...noTotalJobs } = fixtures.dashboardStats
    expect(DashboardStatsSchema.safeParse(noTotalJobs).success).toBe(false)
  })
})

describe('API Schema: Queue Depth', () => {
  it('validates queue depth', () => {
    expect(QueueDepthSchema.safeParse(fixtures.queueDepth).success).toBe(true)
  })
})

describe('API Schema: Jobs', () => {
  it('validates a single job', () => {
    expect(JobSchema.safeParse(fixtures.job).success).toBe(true)
  })

  it('validates paginated jobs response', () => {
    const paginated = { jobs: [fixtures.job], total: 1 }
    expect(PaginatedJobsSchema.safeParse(paginated).success).toBe(true)
  })

  it('validates empty paginated jobs', () => {
    expect(PaginatedJobsSchema.safeParse({ jobs: [], total: 0 }).success).toBe(true)
  })

  it('validates job detail', () => {
    expect(JobDetailSchema.safeParse(fixtures.jobDetail).success).toBe(true)
  })

  it('validates job detail with required prompt field', () => {
    const { prompt: _, ...noPrompt } = fixtures.jobDetail
    expect(JobDetailSchema.safeParse(noPrompt).success).toBe(false)
  })
})

describe('API Schema: Performance', () => {
  it('validates performance stats', () => {
    expect(PerformanceStatsSchema.safeParse(fixtures.performanceStats).success).toBe(true)
  })

  it('validates hourly throughput entry', () => {
    expect(HourlyThroughputSchema.safeParse(fixtures.performanceStats.hourly[0]).success).toBe(true)
  })

  it('validates performance with empty hourly', () => {
    expect(PerformanceStatsSchema.safeParse({ ...fixtures.performanceStats, hourly: [] }).success).toBe(true)
  })
})

describe('API Schema: Analytics', () => {
  it('validates analytics stats', () => {
    expect(AnalyticsStatsSchema.safeParse(fixtures.analyticsStats).success).toBe(true)
  })

  it('validates analytics with empty models and finish_reasons', () => {
    expect(AnalyticsStatsSchema.safeParse({ ...fixtures.analyticsStats, models: [], finish_reasons: [] }).success).toBe(true)
  })
})

describe('API Schema: Lab Settings', () => {
  it('validates lab settings', () => {
    expect(LabSettingsSchema.safeParse(fixtures.labSettings).success).toBe(true)
  })
})

describe('API Schema: Error', () => {
  it('validates error response', () => {
    expect(ApiErrorSchema.safeParse({ error: 'url is required for ollama provider' }).success).toBe(true)
  })
})
