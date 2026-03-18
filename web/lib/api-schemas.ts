/**
 * Zod schemas for API response validation.
 *
 * Derived from:
 * - OpenAPI spec: crates/veronex/src/infrastructure/inbound/http/openapi.json
 * - TypeScript types: lib/types.ts
 * - E2E shape assertions: e2e/api-*.spec.ts
 *
 * These schemas can be used in integration tests to validate real API responses
 * without duplicating manual typeof checks across E2E specs.
 */

import { z } from 'zod'

// ── SSE stream payloads ──────────────────────────────────────────────────────

const NonNegativeInt = z.number().int().nonnegative()

export const JobStatusEventSchema = z.object({
  id: z.string(),
  status: z.string(),
  model_name: z.string(),
  provider_type: z.string(),
  latency_ms: z.number().int().nullable(),
  /** Server-side unix ms timestamp. Used for accurate "time ago" display. */
  ts: z.number().optional(),
})

export const FlowStatsSchema = z.object({
  incoming: NonNegativeInt,
  incoming_60s: NonNegativeInt,
  queued: NonNegativeInt,
  running: NonNegativeInt,
  completed: NonNegativeInt,
})

// ── Enums ───────────────────────────────────────────────────────────────────

export const JobStatusSchema = z.enum([
  'pending', 'running', 'completed', 'failed', 'cancelled',
])

export const JobSourceSchema = z.enum(['api', 'api_paid', 'test', 'analyzer'])

export const ProviderTypeSchema = z.enum(['ollama', 'gemini'])

export const LlmProviderStatusSchema = z.enum(['online', 'degraded', 'offline'])

export const AccountRoleSchema = z.enum(['super', 'admin'])

// ── Keys ────────────────────────────────────────────────────────────────────

export const ApiKeySchema = z.object({
  id: z.string().uuid(),
  key_prefix: z.string(),
  name: z.string(),
  tenant_id: z.string(),
  is_active: z.boolean(),
  rate_limit_rpm: z.number().int(),
  rate_limit_tpm: z.number().int(),
  created_at: z.string(),
  expires_at: z.string().nullable(),
  tier: z.enum(['free', 'paid']),
  created_by: z.string().optional(),
})

export const ApiKeyListSchema = z.array(ApiKeySchema)

export const CreateKeyResponseSchema = z.object({
  id: z.string().uuid(),
  key: z.string(),
  key_prefix: z.string(),
  tenant_id: z.string(),
  created_at: z.string(),
})

// ── Providers ───────────────────────────────────────────────────────────────

export const ProviderSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  provider_type: ProviderTypeSchema,
  url: z.string(),
  is_active: z.boolean(),
  total_vram_mb: z.number().int(),
  gpu_index: z.number().int().nullable(),
  server_id: z.string().uuid().nullable(),
  is_free_tier: z.boolean(),
  status: LlmProviderStatusSchema,
  registered_at: z.string(),
  api_key_masked: z.string().nullable(),
})

export const ProviderListSchema = z.array(ProviderSchema)

export const RegisterProviderResponseSchema = z.object({
  id: z.string().uuid(),
  status: z.string(),
})

// ── Accounts ────────────────────────────────────────────────────────────────

export const RoleInfoSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
})

export const AccountSchema = z.object({
  id: z.string().uuid(),
  username: z.string(),
  name: z.string(),
  email: z.string().nullable(),
  roles: z.array(RoleInfoSchema),
  role_name: z.string(),
  permissions: z.array(z.string()),
  menus: z.array(z.string()),
  department: z.string().nullable(),
  position: z.string().nullable(),
  is_active: z.boolean(),
  last_login_at: z.string().nullable(),
  created_at: z.string(),
})

export const AccountListSchema = z.array(AccountSchema)

export const CreateAccountResponseSchema = z.object({
  id: z.string().uuid(),
  username: z.string(),
  test_api_key: z.string(),
  created_at: z.string(),
})

// ── Servers ─────────────────────────────────────────────────────────────────

export const GpuServerSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  node_exporter_url: z.string().nullable(),
  registered_at: z.string(),
})

export const GpuServerListSchema = z.array(GpuServerSchema)

// ── Dashboard Stats ─────────────────────────────────────────────────────────

export const DashboardStatsSchema = z.object({
  total_keys: z.number().int(),
  active_keys: z.number().int(),
  total_jobs: z.number().int(),
  jobs_last_24h: z.number().int(),
  jobs_by_status: z.record(z.string(), z.number().int()),
})

// ── Queue Depth ─────────────────────────────────────────────────────────────

export const QueueDepthSchema = z.object({
  api_paid: z.number().int(),
  api: z.number().int(),
  test: z.number().int(),
  total: z.number().int(),
})

// ── Jobs ────────────────────────────────────────────────────────────────────

export const JobSchema = z.object({
  id: z.string().uuid(),
  model_name: z.string(),
  provider_type: z.string(),
  status: JobStatusSchema,
  source: JobSourceSchema,
  created_at: z.string(),
  completed_at: z.string().nullable(),
  latency_ms: z.number().nullable(),
  ttft_ms: z.number().nullable(),
  prompt_tokens: z.number().int().nullable(),
  completion_tokens: z.number().int().nullable(),
  cached_tokens: z.number().int().nullable(),
  tps: z.number().nullable(),
  api_key_name: z.string().nullable(),
  account_name: z.string().nullable(),
  request_path: z.string().nullable(),
  estimated_cost_usd: z.number().nullable(),
  has_tool_calls: z.boolean(),
  provider_name: z.string().nullable().optional(),
})

export const PaginatedJobsSchema = z.object({
  jobs: z.array(JobSchema),
  total: z.number().int(),
})

export const JobDetailSchema = z.object({
  id: z.string().uuid(),
  model_name: z.string(),
  provider_type: z.string(),
  status: JobStatusSchema,
  source: JobSourceSchema,
  created_at: z.string(),
  completed_at: z.string().nullable(),
  started_at: z.string().nullable(),
  latency_ms: z.number().nullable(),
  ttft_ms: z.number().nullable(),
  prompt_tokens: z.number().int().nullable(),
  completion_tokens: z.number().int().nullable(),
  cached_tokens: z.number().int().nullable(),
  tps: z.number().nullable(),
  api_key_name: z.string().nullable(),
  account_name: z.string().nullable(),
  request_path: z.string().nullable(),
  estimated_cost_usd: z.number().nullable(),
  prompt: z.string(),
  result_text: z.string().nullable(),
  error: z.string().nullable(),
  tool_calls_json: z.array(z.unknown()).nullable(),
  message_count: z.number().int().nullable(),
  messages_json: z.array(z.unknown()).nullable(),
  provider_name: z.string().nullable().optional(),
  image_keys: z.array(z.string()).nullable().optional(),
  image_urls: z.array(z.string()).nullable().optional(),
})

// ── Performance ─────────────────────────────────────────────────────────────

export const HourlyThroughputSchema = z.object({
  hour: z.string(),
  request_count: z.number().int(),
  success_count: z.number().int(),
  avg_latency_ms: z.number(),
  total_tokens: z.number().int(),
})

export const PerformanceStatsSchema = z.object({
  avg_latency_ms: z.number(),
  p50_latency_ms: z.number(),
  p95_latency_ms: z.number(),
  p99_latency_ms: z.number(),
  total_requests: z.number().int(),
  success_rate: z.number(),
  total_tokens: z.number().int(),
  hourly: z.array(HourlyThroughputSchema),
})

// ── Analytics ───────────────────────────────────────────────────────────────

export const ModelStatSchema = z.object({
  model_name: z.string(),
  request_count: z.number().int(),
  success_count: z.number().int(),
  success_rate: z.number(),
  total_prompt_tokens: z.number().int(),
  total_completion_tokens: z.number().int(),
  avg_latency_ms: z.number(),
})

export const FinishReasonStatSchema = z.object({
  reason: z.string(),
  count: z.number().int(),
})

export const AnalyticsStatsSchema = z.object({
  avg_tps: z.number(),
  avg_prompt_tokens: z.number(),
  avg_completion_tokens: z.number(),
  models: z.array(ModelStatSchema),
  finish_reasons: z.array(FinishReasonStatSchema),
})

// ── Lab Settings ────────────────────────────────────────────────────────────

export const LabSettingsSchema = z.object({
  gemini_function_calling: z.boolean(),
  updated_at: z.string(),
})

// ── Roles ────────────────────────────────────────────────────────────────────

export const RoleSummarySchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  permissions: z.array(z.string()),
  menus: z.array(z.string()),
  is_system: z.boolean(),
  account_count: z.number().int(),
  created_at: z.string(),
})

export const RoleSummaryListSchema = z.array(RoleSummarySchema)

// ── Login ────────────────────────────────────────────────────────────────────

export const LoginResponseSchema = z.object({
  ok: z.boolean(),
  account_id: z.string(),
  username: z.string(),
  role: z.string(),
  permissions: z.array(z.string()),
  menus: z.array(z.string()),
})

// ── Error ───────────────────────────────────────────────────────────────────

export const ApiErrorSchema = z.object({
  error: z.string(),
})
