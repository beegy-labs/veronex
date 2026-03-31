import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Usage', () => {
  let api: ReturnType<typeof authedRequest>
  let accountId: string

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
    accountId = tokens.accountId
  })

  test('aggregate usage returns expected shape', async () => {
    const res = await api.get('/v1/usage?hours=24')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.request_count).toBe('number')
    expect(typeof body.success_count).toBe('number')
    expect(typeof body.error_count).toBe('number')
    expect(typeof body.prompt_tokens).toBe('number')
    expect(typeof body.completion_tokens).toBe('number')
    expect(typeof body.total_tokens).toBe('number')
  })

  test('usage breakdown returns provider, key, and model arrays', async () => {
    const res = await api.get('/v1/usage/breakdown?hours=24')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.by_providers)).toBeTruthy()
    expect(Array.isArray(body.by_key)).toBeTruthy()
    expect(Array.isArray(body.by_model)).toBeTruthy()
    expect(typeof body.total_cost_usd).toBe('number')
  })

  test('per-key hourly usage returns array (with owned key)', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        tenant_id: accountId,
        name: `e2e-usage-${testId()}`,
        tier: 'free',
      })
      expect(createRes.ok()).toBeTruthy()
      const { id } = await createRes.json()
      keyId = id

      const res = await api.get(`/v1/usage/${id}?hours=24`)
      expect(res.ok()).toBeTruthy()
      const body = await res.json()
      expect(Array.isArray(body)).toBeTruthy()
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })

  test('per-key model breakdown returns array', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        tenant_id: accountId,
        name: `e2e-usage-models-${testId()}`,
        tier: 'free',
      })
      expect(createRes.ok()).toBeTruthy()
      const { id } = await createRes.json()
      keyId = id

      const res = await api.get(`/v1/usage/${id}/models?hours=24`)
      expect(res.ok()).toBeTruthy()
      const body = await res.json()
      expect(Array.isArray(body)).toBeTruthy()
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })

  test('per-key jobs returns array', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        tenant_id: accountId,
        name: `e2e-usage-jobs-${testId()}`,
        tier: 'free',
      })
      expect(createRes.ok()).toBeTruthy()
      const { id } = await createRes.json()
      keyId = id

      const res = await api.get(`/v1/usage/${id}/jobs?hours=24`)
      expect(res.ok()).toBeTruthy()
      const body = await res.json()
      expect(Array.isArray(body)).toBeTruthy()
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })

  test('usage for non-existent key returns 404', async () => {
    const fakeId = '00000000-0000-0000-0000-000000000000'
    const res = await api.get(`/v1/usage/${fakeId}?hours=24`)
    expect(res.status()).toBe(404)
  })
})
