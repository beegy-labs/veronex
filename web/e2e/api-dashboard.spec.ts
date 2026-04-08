import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: Dashboard & Inference @smoke', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  // ── Dashboard Stats ─────────────────────────────────────────────────────────
  test('dashboard stats returns expected shape', async () => {
    const res = await api.get('/v1/dashboard/stats')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    // Should have numeric counts matching DashboardStats struct
    expect(typeof body.total_jobs).toBe('number')
    expect(typeof body.active_keys).toBe('number')
    expect(typeof body.total_keys).toBe('number')
  })

  // ── Queue Depth ─────────────────────────────────────────────────────────────
  test('queue depth returns expected shape', async () => {
    const res = await api.get('/v1/dashboard/queue/depth')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.api_paid).toBe('number')
    expect(typeof body.api).toBe('number')
    expect(typeof body.test).toBe('number')
    expect(typeof body.total).toBe('number')
  })

  // ── Jobs List ───────────────────────────────────────────────────────────────
  test('jobs list returns paginated response', async () => {
    const res = await api.get('/v1/dashboard/jobs?limit=10')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.jobs)).toBeTruthy()
    expect(typeof body.total).toBe('number')
  })

  // ── Capacity ────────────────────────────────────────────────────────────────
  test('capacity returns provider slot info', async () => {
    const res = await api.get('/v1/dashboard/capacity')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(body).toBeTruthy()
  })

  // ── Dashboard Overview ──────────────────────────────────────────────────────
  test('dashboard overview returns aggregated snapshot', async () => {
    const res = await api.get('/v1/dashboard/overview')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.stats).toBe('object')
    expect(typeof body.queue_depth).toBe('object')
    expect(typeof body.lab).toBe('object')
  })

  // ── Capacity Cluster ────────────────────────────────────────────────────────
  test('capacity cluster returns array', async () => {
    const res = await api.get('/v1/dashboard/capacity/cluster')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  // ── Service Health ──────────────────────────────────────────────────────────
  test('service health returns infrastructure and pod status', async () => {
    const res = await api.get('/v1/dashboard/services')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.infrastructure)).toBeTruthy()
    expect(Array.isArray(body.api_pods)).toBeTruthy()
    expect(Array.isArray(body.agent_pods)).toBeTruthy()

    // At least one API pod should be online (the instance serving this request)
    expect(body.api_pods.length).toBeGreaterThan(0)
    for (const pod of body.api_pods) {
      expect(pod.id).toBeTruthy()
      expect(['online', 'offline']).toContain(pod.status)
    }

    // Infrastructure services should have valid status values
    for (const svc of body.infrastructure) {
      expect(svc.name).toBeTruthy()
      expect(['ok', 'degraded', 'unavailable']).toContain(svc.status)
    }
  })

  // ── Lab Settings ────────────────────────────────────────────────────────────
  test.describe.serial('lab settings', () => {
    test('lab settings CRUD', async () => {
      // Get current settings
      const getRes = await api.get('/v1/dashboard/lab')
      expect(getRes.ok()).toBeTruthy()
      const settings = await getRes.json()
      expect(typeof settings.gemini_function_calling).toBe('boolean')

      // Patch — toggle gemini_function_calling and revert
      const current = settings.gemini_function_calling
      try {
        const patchRes = await api.patch('/v1/dashboard/lab', {
          gemini_function_calling: !current,
        })
        expect(patchRes.ok()).toBeTruthy()
      } finally {
        // Always revert to original state
        await api.patch('/v1/dashboard/lab', {
          gemini_function_calling: current,
        })
      }
    })
  })
})
