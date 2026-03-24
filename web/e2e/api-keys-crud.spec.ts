import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Keys CRUD @smoke', () => {
  let api: ReturnType<typeof authedRequest>
  let accountId: string

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
    accountId = tokens.accountId
  })

  test('list keys returns array', async () => {
    const res = await api.get('/v1/keys')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    // Response is paginated: {keys: [...], page, limit, total}
    expect(Array.isArray(body.keys)).toBeTruthy()
  })

  test('create, toggle, and delete key lifecycle', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        tenant_id: accountId,
        name: `e2e-lifecycle-${testId()}`,
        tier: 'free',
      })
      expect(createRes.ok()).toBeTruthy()
      const created = await createRes.json()
      keyId = created.id
      expect(created.id).toBeTruthy()
      expect(created.key).toMatch(/^vnx_/)

      // Toggle inactive
      const toggleRes = await api.patch(`/v1/keys/${created.id}`, { is_active: false })
      expect(toggleRes.ok()).toBeTruthy()

      // Toggle back active
      const toggleRes2 = await api.patch(`/v1/keys/${created.id}`, { is_active: true })
      expect(toggleRes2.ok()).toBeTruthy()

      // Delete
      const deleteRes = await api.delete(`/v1/keys/${created.id}`)
      expect([200, 204]).toContain(deleteRes.status())

      // Verify deleted key no longer in list
      const listRes = await api.get('/v1/keys')
      const { keys } = await listRes.json()
      expect(keys.find((k: { id: string }) => k.id === created.id)).toBeUndefined()
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })

  test('create key with rate limits', async () => {
    let keyId: string | undefined
    try {
      const createRes = await api.post('/v1/keys', {
        tenant_id: accountId,
        name: `e2e-ratelimit-${testId()}`,
        tier: 'paid',
        rate_limit_rpm: 100,
        rate_limit_tpm: 50000,
      })
      expect(createRes.ok()).toBeTruthy()
      const { id } = await createRes.json()
      keyId = id
    } finally {
      if (keyId) await api.delete(`/v1/keys/${keyId}`)
    }
  })
})
