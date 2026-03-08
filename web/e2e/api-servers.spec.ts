import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: GPU Servers', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list servers returns array', async () => {
    const res = await api.get('/v1/servers')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  test('server CRUD lifecycle', async () => {
    const name = `e2e-server-${testId()}`
    let serverId: string | undefined
    try {
      // Register
      const createRes = await api.post('/v1/servers', {
        name,
        hostname: '192.168.1.99',
      })
      expect(createRes.status()).toBe(201)
      const { id } = await createRes.json()
      serverId = id
      expect(id).toBeTruthy()

      // Update
      const updateRes = await api.patch(`/v1/servers/${id}`, {
        name: `${name}-updated`,
        node_exporter_url: 'http://192.168.1.99:9100/metrics',
      })
      expect(updateRes.ok()).toBeTruthy()
      const updated = await updateRes.json()
      expect(updated.name).toBe(`${name}-updated`)

      // Delete
      const deleteRes = await api.delete(`/v1/servers/${id}`)
      expect(deleteRes.status()).toBe(204)
    } finally {
      if (serverId) await api.delete(`/v1/servers/${serverId}`)
    }
  })

  test('delete non-existent server returns 404', async () => {
    const res = await api.delete('/v1/servers/00000000-0000-0000-0000-000000000000')
    expect(res.status()).toBe(404)
  })
})
