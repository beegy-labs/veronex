import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Keys CRUD @smoke', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list keys returns array', async () => {
    const res = await api.get('/v1/keys')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  test('create, toggle, and delete key lifecycle', async () => {
    let keyId: string | undefined
    try {
      // Create
      const createRes = await api.post('/v1/keys', {
        name: `e2e-lifecycle-${testId()}`,
        tier: 'free',
      })
      expect(createRes.status()).toBe(201)
      const { id, raw_key } = await createRes.json()
      keyId = id
      expect(id).toBeTruthy()
      expect(raw_key).toMatch(/^sk-/)

      // Toggle inactive
      const toggleRes = await api.patch(`/v1/keys/${id}`, { is_active: false })
      expect(toggleRes.ok()).toBeTruthy()

      // Toggle back active
      const toggleRes2 = await api.patch(`/v1/keys/${id}`, { is_active: true })
      expect(toggleRes2.ok()).toBeTruthy()

      // Delete
      const deleteRes = await api.delete(`/v1/keys/${id}`)
      expect(deleteRes.status()).toBe(204)

      // Verify deleted key no longer in list
      const listRes = await api.get('/v1/keys')
      const keys = await listRes.json()
      expect(keys.find((k: { id: string }) => k.id === id)).toBeUndefined()
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })

  test('create key with rate limits', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        name: `e2e-ratelimit-${testId()}`,
        tier: 'paid',
        rate_limit_rpm: 100,
        rate_limit_tpm: 50000,
      })
      expect(createRes.status()).toBe(201)
      const { id } = await createRes.json()
      keyId = id
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })
})
