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
}

export interface Job {
  id: string
  model_name: string
  backend: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  created_at: string
  completed_at: string | null
  latency_ms: number | null
}

export interface DashboardStats {
  total_keys: number
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

export interface HourlyThroughput {
  hour: string
  request_count: number
  success_count: number
  avg_latency_ms: number
  total_tokens: number
}

export interface CreateKeyRequest {
  name: string
  tenant_id: string
  rate_limit_rpm?: number
  rate_limit_tpm?: number
}

export interface Backend {
  id: string
  name: string
  backend_type: 'ollama' | 'gemini'
  url: string
  is_active: boolean
  total_vram_mb: number
  status: 'online' | 'offline' | 'degraded'
  registered_at: string
}

export interface RegisterBackendRequest {
  name: string
  backend_type: 'ollama' | 'gemini'
  url?: string
  api_key?: string
  total_vram_mb?: number
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
