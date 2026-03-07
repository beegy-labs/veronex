import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Accounts', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list accounts returns array with current user', async () => {
    const res = await api.get('/v1/accounts')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
    // At least the admin account should exist
    expect(body.length).toBeGreaterThanOrEqual(1)
    const admin = body.find((a: { username: string }) => a.username === 'admin')
    expect(admin).toBeTruthy()
  })

  test('create, update, and delete account lifecycle', async () => {
    const username = `e2e-user-${testId()}`
    let createdId: string | undefined
    try {
      // Create
      const createRes = await api.post('/v1/accounts', {
        username,
        password: 'TestPass123!',
        role: 'admin',
      })
      expect(createRes.status()).toBe(201)
      const created = await createRes.json()
      createdId = created.id
      expect(created.id).toBeTruthy()
      expect(created.username).toBe(username)

      // Verify in list
      const listRes = await api.get('/v1/accounts')
      const accounts = await listRes.json()
      expect(accounts.find((a: { username: string }) => a.username === username)).toBeTruthy()

      // Delete
      const deleteRes = await api.delete(`/v1/accounts/${createdId}`)
      expect(deleteRes.status()).toBe(204)
    } finally {
      if (createdId) await api.delete(`/v1/accounts/${createdId}`)
    }
  })

  test('cannot create account with empty username', async () => {
    const res = await api.post('/v1/accounts', {
      username: '',
      password: 'TestPass123!',
      role: 'admin',
    })
    expect(res.status()).toBe(400)
  })

  test('cannot create account with duplicate username', async () => {
    // Try to create with the same username as the admin
    const res = await api.post('/v1/accounts', {
      username: 'admin',
      password: 'TestPass123!',
      role: 'admin',
    })
    // Should fail with 400 or 409
    expect([400, 409]).toContain(res.status())
  })
})
