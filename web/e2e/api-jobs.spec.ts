import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: Jobs & Performance', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  // ── Job detail ────────────────────────────────────────────────────

  test('get job detail for non-existent job returns 404', async () => {
    const fakeId = 'job_0000000000000000000000'
    const res = await api.get(`/v1/dashboard/jobs/${fakeId}`)
    expect(res.status()).toBe(404)
  })

  test('get job detail returns expected shape when job exists', async () => {
    // First list jobs to find one (if any)
    const listRes = await api.get('/v1/dashboard/jobs?limit=1')
    expect(listRes.ok()).toBeTruthy()
    const { jobs } = await listRes.json()

    if (jobs.length === 0) {
      // No jobs exist — skip shape validation
      return
    }

    const jobId = jobs[0].id
    const res = await api.get(`/v1/dashboard/jobs/${jobId}`)
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(body.id).toBe(jobId)
    expect(typeof body.model_name).toBe('string')
    expect(typeof body.status).toBe('string')
    expect(typeof body.created_at).toBe('string')
  })

  // ── Cancel job ────────────────────────────────────────────────────

  test('cancel non-existent job is idempotent', async () => {
    const fakeId = 'job_0000000000000000000000'
    const res = await api.delete(`/v1/dashboard/jobs/${fakeId}`)
    // Cancel is idempotent — returns 200 or 204 even for non-existent jobs
    expect([200, 204, 404]).toContain(res.status())
  })

  // ── Performance ───────────────────────────────────────────────────

  test('performance returns expected shape', async () => {
    const res = await api.get('/v1/dashboard/performance?hours=24')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.avg_latency_ms).toBe('number')
    expect(typeof body.p50_latency_ms).toBe('number')
    expect(typeof body.p95_latency_ms).toBe('number')
    expect(typeof body.p99_latency_ms).toBe('number')
    expect(typeof body.total_requests).toBe('number')
    expect(typeof body.success_rate).toBe('number')
    expect(typeof body.total_tokens).toBe('number')
    expect(Array.isArray(body.hourly)).toBeTruthy()
  })

  test('performance hourly entries have expected shape', async () => {
    const res = await api.get('/v1/dashboard/performance?hours=24')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()

    if (body.hourly.length > 0) {
      const entry = body.hourly[0]
      expect(typeof entry.hour).toBe('string')
      expect(typeof entry.request_count).toBe('number')
      expect(typeof entry.success_count).toBe('number')
      expect(typeof entry.avg_latency_ms).toBe('number')
      expect(typeof entry.total_tokens).toBe('number')
    }
  })

  // ── Analytics ─────────────────────────────────────────────────────

  test('analytics summary returns expected shape', async () => {
    const res = await api.get('/v1/dashboard/analytics?hours=24')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.avg_tps).toBe('number')
    expect(typeof body.avg_prompt_tokens).toBe('number')
    expect(typeof body.avg_completion_tokens).toBe('number')
    expect(Array.isArray(body.models)).toBeTruthy()
    expect(Array.isArray(body.finish_reasons)).toBeTruthy()
  })
})
