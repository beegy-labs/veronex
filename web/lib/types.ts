export interface ApiKey {
  id: string
  key_prefix: string
  name: string
  tenant_id: string
  is_active: boolean
  rate_limit_rpm: number
  rate_limit_tpm: number
  created_at: string
  expires_at: string | null
  /** Billing tier: `"free"` or `"paid"` */
  tier: 'free' | 'paid'
}

export interface Job {
  id: string
  model_name: string
  backend: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  source: 'api' | 'test'
  created_at: string
  completed_at: string | null
  latency_ms: number | null
  ttft_ms: number | null
  prompt_tokens: number | null
  completion_tokens: number | null
  cached_tokens: number | null
  tps: number | null
  api_key_name: string | null
  /** For test run jobs: the account name of who ran it. */
  account_name: string | null
  /** HTTP path the request arrived via, e.g. "/v1/chat/completions" */
  request_path: string | null
}

export interface JobDetail {
  id: string
  model_name: string
  backend: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  source: 'api' | 'test'
  created_at: string
  started_at: string | null
  completed_at: string | null
  latency_ms: number | null
  ttft_ms: number | null
  prompt_tokens: number | null
  completion_tokens: number | null
  cached_tokens: number | null
  tps: number | null
  api_key_name: string | null
  /** For test run jobs: the account name of who ran it. */
  account_name: string | null
  prompt: string
  result_text: string | null
  error: string | null
  /** HTTP path the request arrived via, e.g. "/v1/chat/completions" */
  request_path: string | null
}

export interface DashboardStats {
  total_keys: number
  /** Active standard (non-test) keys */
  active_keys: number
  total_jobs: number
  jobs_last_24h: number
  jobs_by_status: Record<string, number>
}

export interface UsageAggregate {
  request_count: number
  success_count: number
  cancelled_count: number
  error_count: number
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
}

export interface HourlyUsage {
  hour: string
  request_count: number
  success_count: number
  cancelled_count: number
  error_count: number
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
}

export interface PerformanceStats {
  avg_latency_ms: number
  p50_latency_ms: number
  p95_latency_ms: number
  p99_latency_ms: number
  total_requests: number
  success_rate: number
  total_tokens: number
  hourly: HourlyThroughput[]
}

export interface ModelStat {
  model_name: string
  request_count: number
  success_count: number
  success_rate: number
  total_prompt_tokens: number
  total_completion_tokens: number
  avg_latency_ms: number
}

export interface FinishReasonStat {
  reason: string
  count: number
}

export interface AnalyticsStats {
  avg_tps: number
  avg_prompt_tokens: number
  avg_completion_tokens: number
  models: ModelStat[]
  finish_reasons: FinishReasonStat[]
}

export interface HourlyThroughput {
  hour: string
  request_count: number
  success_count: number
  avg_latency_ms: number
  total_tokens: number
}

export interface BackendBreakdown {
  backend: string
  request_count: number
  success_count: number
  error_count: number
  prompt_tokens: number
  completion_tokens: number
  success_rate: number
}

export interface KeyBreakdown {
  key_id: string
  key_name: string
  key_prefix: string
  request_count: number
  success_count: number
  prompt_tokens: number
  completion_tokens: number
  success_rate: number
}

export interface ModelBreakdown {
  model_name: string
  backend: string
  request_count: number
  call_pct: number
  prompt_tokens: number
  completion_tokens: number
  avg_latency_ms: number
}

export interface UsageBreakdown {
  by_backend: BackendBreakdown[]
  by_key: KeyBreakdown[]
  by_model: ModelBreakdown[]
}

export interface CreateKeyRequest {
  name: string
  tenant_id: string
  rate_limit_rpm?: number
  rate_limit_tpm?: number
  /** Billing tier: `"free"` or `"paid"` (default) */
  tier?: 'free' | 'paid'
}

export interface GpuServer {
  id: string
  name: string
  node_exporter_url: string | null
  registered_at: string
}

export interface RegisterGpuServerRequest {
  name: string
  node_exporter_url?: string
}

export interface UpdateGpuServerRequest {
  name?: string
  node_exporter_url?: string
}

export interface NodeMetrics {
  scrape_ok: boolean
  mem_total_mb: number
  mem_available_mb: number
  /** Logical CPUs (hardware threads) */
  cpu_logical: number
  /** Physical cores — null when node_cpu_info is not available */
  cpu_physical: number | null
  /** Instantaneous CPU usage 0–100 %. null on first scrape (no delta yet) */
  cpu_usage_pct: number | null
  gpus: GpuNodeMetrics[]
}

export interface GpuNodeMetrics {
  card: string
  temp_c: number | null
  power_w: number | null
  vram_used_mb: number | null
  vram_total_mb: number | null
  busy_pct: number | null
}

export interface Backend {
  id: string
  name: string
  backend_type: 'ollama' | 'gemini'
  url: string
  is_active: boolean
  total_vram_mb: number
  /** GPU index on the host (0-based). Used to filter node-exporter metrics. */
  gpu_index: number | null
  /** FK → gpu_servers. null for cloud backends. */
  server_id: string | null
  /** Reserved for Phase 2 sidecar. */
  agent_url: string | null
  /** true = Google free-tier project; RPM/RPD limits come from gemini_rate_limit_policies. */
  is_free_tier: boolean
  status: 'online' | 'offline' | 'degraded'
  registered_at: string
  /** Masked API key shown in the UI (e.g. `AIza...x1y2`). Gemini only. */
  api_key_masked: string | null
}

export interface RegisterBackendRequest {
  name: string
  backend_type: 'ollama' | 'gemini'
  url?: string
  api_key?: string
  total_vram_mb?: number
  gpu_index?: number
  server_id?: string
  agent_url?: string
  is_free_tier?: boolean
}

export interface UpdateBackendRequest {
  name: string
  url?: string
  api_key?: string
  total_vram_mb?: number
  gpu_index?: number | null
  server_id?: string | null
  is_free_tier?: boolean
  is_active?: boolean
}

export interface GeminiStatusResult {
  id: string
  name: string
  status: 'online' | 'offline' | 'degraded'
  error: string | null
}

export interface GeminiStatusSyncResponse {
  synced_at: string
  results: GeminiStatusResult[]
}

/** Per-model Gemini rate-limit policy. model_name="*" = global fallback. */
export interface GeminiRateLimitPolicy {
  id: string
  model_name: string
  rpm_limit: number
  rpd_limit: number
  /**
   * When false: skip all free-tier backends and route directly to a paid backend.
   * RPM/RPD counters are also suppressed for paid backends.
   */
  available_on_free_tier: boolean
  updated_at: string
}

export interface UpsertGeminiPolicyRequest {
  rpm_limit: number
  rpd_limit: number
  available_on_free_tier: boolean
}

export interface ServerMetricsPoint {
  ts: string
  mem_total_mb: number
  mem_avail_mb: number
  gpu_temp_c: number | null
  gpu_power_w: number | null
}

export interface BackendSelectedModel {
  model_name: string
  is_enabled: boolean
  synced_at: string
}

export interface GeminiSyncConfig {
  api_key_masked: string | null
}

export interface GeminiModel {
  model_name: string
  synced_at: string
}

export interface RegisterBackendResponse {
  id: string
  status: string
}

export interface CreateKeyResponse {
  id: string
  key: string
  key_prefix: string
  tenant_id: string
  created_at: string
}

export interface OllamaSyncResult {
  backend_id: string
  name: string
  models: string[]
  error: string | null
}

export interface OllamaSyncJob {
  id: string
  started_at: string
  completed_at: string | null
  status: 'running' | 'completed'
  total_backends: number
  done_backends: number
  results: OllamaSyncResult[]
}

/** Model with count of backends that carry it (from GET /v1/ollama/models). */
export interface OllamaModelWithCount {
  model_name: string
  backend_count: number
}

/** Backend info returned by GET /v1/ollama/models/:model_name/backends. */
export interface RetryParams {
  prompt: string
  model: string
  backend: string
}

export interface OllamaBackendForModel {
  backend_id: string
  name: string
  url: string
  status: string
}

export interface Account {
  id: string
  username: string
  name: string
  email: string | null
  role: 'super' | 'admin'
  department: string | null
  position: string | null
  is_active: boolean
  last_login_at: string | null
  created_at: string
}

export interface CreateAccountRequest {
  username: string
  password: string
  name: string
  email?: string
  role?: string
  department?: string
  position?: string
}

export interface CreateAccountResponse {
  id: string
  username: string
  role: string
  test_api_key: string
  created_at: string
}

export interface LoginRequest {
  username: string
  password: string
}

export interface LoginResponse {
  access_token: string
  token_type: string
  account_id: string
  username: string
  role: string
  refresh_token: string
}

export interface SessionRecord {
  id: string
  ip_address: string | null
  created_at: string
  last_used_at: string | null
  expires_at: string
}

export interface ModelCapacityInfo {
  model_name: string
  recommended_slots: number
  active_slots: number
  available_slots: number
  vram_model_mb: number
  vram_kv_per_slot_mb: number
  avg_tokens_per_sec: number
  p95_latency_ms: number
  sample_count: number
  llm_concern: string | null
  llm_reason: string | null
  updated_at: string
}

export interface BackendCapacityInfo {
  backend_id: string
  backend_name: string
  thermal_state: 'normal' | 'soft' | 'hard'
  temp_c: number | null
  models: ModelCapacityInfo[]
}

export interface CapacityResponse {
  backends: BackendCapacityInfo[]
}

export interface CapacitySettings {
  analyzer_model: string
  batch_enabled: boolean
  batch_interval_secs: number
  last_run_at: string | null
  last_run_status: string | null
  available_models: string[]
}

export interface PatchCapacitySettings {
  analyzer_model?: string
  batch_enabled?: boolean
  batch_interval_secs?: number
}

export interface AuditEvent {
  event_time: string
  account_id: string
  account_name: string
  action: string
  resource_type: string
  resource_id: string
  resource_name: string
  ip_address: string
  details: string
}
