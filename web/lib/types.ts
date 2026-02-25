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

export interface NodeMetrics {
  scrape_ok: boolean
  mem_total_mb: number
  mem_available_mb: number
  cpu_cores: number
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
  status: 'online' | 'offline' | 'degraded'
  registered_at: string
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
}

export interface UpdateBackendRequest {
  name: string
  url?: string
  api_key?: string
  total_vram_mb?: number
  gpu_index?: number | null
  server_id?: string | null
}

export interface ServerMetricsPoint {
  ts: string
  mem_total_mb: number
  mem_avail_mb: number
  gpu_temp_c: number | null
  gpu_power_w: number | null
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
