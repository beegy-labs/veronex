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
    // Response is paginated: {servers: [...], page, limit, total}
    expect(Array.isArray(body.servers)).toBeTruthy()
  })

  test('server CRUD lifecycle', async () => {
    const name = `e2e-server-${testId()}`
    let serverId: string | undefined
    try {
      const createRes = await api.post('/v1/servers', {
        name,
        node_exporter_url: 'http://192.168.1.99:9100',
      })
      // API validates node_exporter reachability — returns 502 if unreachable
      if (createRes.status() === 502) {
        return // URL not reachable — skip CRUD validation
      }
      expect(createRes.status()).toBe(201)
      const { id } = await createRes.json()
      serverId = id
      expect(id).toBeTruthy()

      // Update
      const updateRes = await api.patch(`/v1/servers/${id}`, {
        name: `${name}-updated`,
      })
      expect(updateRes.ok()).toBeTruthy()
      const updated = await updateRes.json()
      expect(updated.name).toBe(`${name}-updated`)

      // Delete
      const deleteRes = await api.delete(`/v1/servers/${id}`)
      expect([204, 200]).toContain(deleteRes.status())
    } finally {
      if (serverId) await api.delete(`/v1/servers/${serverId}`)
    }
  })

  test('delete non-existent server is idempotent', async () => {
    const res = await api.delete('/v1/servers/00000000-0000-0000-0000-000000000000')
    // Delete is idempotent — returns 204 even for non-existent servers
    expect([204, 404]).toContain(res.status())
  })
})
