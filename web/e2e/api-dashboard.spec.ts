import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: Dashboard & Inference', () => {
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
      const patchRes = await api.patch('/v1/dashboard/lab', {
        gemini_function_calling: !current,
      })
      expect(patchRes.ok()).toBeTruthy()

      // Revert
      await api.patch('/v1/dashboard/lab', {
        gemini_function_calling: current,
      })
    })
  })
})
