import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: Capacity', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  // ── Capacity overview ─────────────────────────────────────────────

  test('capacity returns providers array', async () => {
    const res = await api.get('/v1/dashboard/capacity')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.providers)).toBeTruthy()
  })

  test('capacity provider entries have expected shape', async () => {
    const res = await api.get('/v1/dashboard/capacity')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()

    if (body.providers.length > 0) {
      const provider = body.providers[0]
      expect(typeof provider.provider_id).toBe('string')
      expect(typeof provider.provider_name).toBe('string')
      expect(typeof provider.total_vram_mb).toBe('number')
      expect(typeof provider.used_vram_mb).toBe('number')
      expect(typeof provider.available_vram_mb).toBe('number')
      expect(typeof provider.thermal_state).toBe('string')
      expect(Array.isArray(provider.loaded_models)).toBeTruthy()
    }
  })

  // ── Capacity settings ─────────────────────────────────────────────

  test('capacity settings returns expected shape', async () => {
    const res = await api.get('/v1/dashboard/capacity/settings')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.analyzer_model).toBe('string')
    expect(typeof body.sync_enabled).toBe('boolean')
    expect(typeof body.sync_interval_secs).toBe('number')
    expect(typeof body.probe_permits).toBe('number')
    expect(typeof body.probe_rate).toBe('number')
    expect(Array.isArray(body.available_models)).toBeTruthy()
  })

  test.describe.serial('capacity settings CRUD', () => {
    test('patch and revert capacity settings', async () => {
      // Read current settings
      const getRes = await api.get('/v1/dashboard/capacity/settings')
      expect(getRes.ok()).toBeTruthy()
      const original = await getRes.json()

      // Toggle sync_enabled
      const newValue = !original.sync_enabled
      const patchRes = await api.patch('/v1/dashboard/capacity/settings', {
        sync_enabled: newValue,
      })
      expect(patchRes.ok()).toBeTruthy()
      const patched = await patchRes.json()
      expect(patched.sync_enabled).toBe(newValue)

      // Revert
      const revertRes = await api.patch('/v1/dashboard/capacity/settings', {
        sync_enabled: original.sync_enabled,
      })
      expect(revertRes.ok()).toBeTruthy()
      const reverted = await revertRes.json()
      expect(reverted.sync_enabled).toBe(original.sync_enabled)
    })
  })
})
