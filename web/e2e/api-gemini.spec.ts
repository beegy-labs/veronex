import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Gemini Policies & Models', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  // ── Gemini Policies ─────────────────────────────────────────────────────────
  test('list gemini policies returns array', async () => {
    const res = await api.get('/v1/gemini/policies')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  test('upsert gemini policy creates or updates policy', async () => {
    const modelName = `e2e-model-${testId()}`
    try {
      const res = await api.put(`/v1/gemini/policies/${modelName}`, {
        rpm_limit: 5,
        rpd_limit: 50,
        available_on_free_tier: true,
      })
      expect(res.ok()).toBeTruthy()
      const body = await res.json()
      expect(body.model_name).toBe(modelName)
      expect(body.rpm_limit).toBe(5)
      expect(body.rpd_limit).toBe(50)
      expect(body.available_on_free_tier).toBe(true)

      // Update the same policy
      const updateRes = await api.put(`/v1/gemini/policies/${modelName}`, {
        rpm_limit: 10,
        rpd_limit: 100,
        available_on_free_tier: false,
      })
      expect(updateRes.ok()).toBeTruthy()
      const updated = await updateRes.json()
      expect(updated.rpm_limit).toBe(10)
      expect(updated.available_on_free_tier).toBe(false)
    } finally {
      await api.delete(`/v1/gemini/policies/${modelName}`)
    }
  })

  // ── Gemini Sync Config ──────────────────────────────────────────────────────
  test('get sync config returns masked key or null', async () => {
    const res = await api.get('/v1/gemini/sync-config')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    // api_key_masked is either null or a masked string
    expect('api_key_masked' in body).toBeTruthy()
  })

  test('set sync config with empty key returns 400', async () => {
    const res = await api.put('/v1/gemini/sync-config', { api_key: '' })
    expect(res.status()).toBe(400)
  })

  // ── Gemini Models ───────────────────────────────────────────────────────────
  test('list gemini models returns models array', async () => {
    const res = await api.get('/v1/gemini/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.models)).toBeTruthy()
  })

  test('sync models without config returns 400', async () => {
    // Only test this if no sync config exists
    const configRes = await api.get('/v1/gemini/sync-config')
    const config = await configRes.json()
    if (!config.api_key_masked) {
      const res = await api.post('/v1/gemini/models/sync')
      expect(res.status()).toBe(400)
    }
  })

  test('sync status without prior sync returns 400 or 200', async () => {
    const res = await api.post('/v1/gemini/sync-status')
    // 400 if no config, 200 if config exists
    expect([200, 400]).toContain(res.status())
  })
})
