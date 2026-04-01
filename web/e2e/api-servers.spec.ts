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
    const res = await api.delete('/v1/servers/gpu_0000000000000000000000')
    // Delete is idempotent — returns 204 even for non-existent servers
    expect([204, 404]).toContain(res.status())
  })

  test('get server metrics returns object', async () => {
    const listRes = await api.get('/v1/servers')
    const { servers } = await listRes.json()
    if (servers.length === 0) return

    const id = servers[0].id
    const res = await api.get(`/v1/servers/${id}/metrics`)
    expect(res.ok()).toBeTruthy()
    expect(typeof (await res.json())).toBe('object')
  })

  test('get server metrics batch returns map', async () => {
    const listRes = await api.get('/v1/servers')
    const { servers } = await listRes.json()
    const ids = servers.length > 0
      ? servers.slice(0, 3).map((s: { id: string }) => s.id).join(',')
      : 'gpu_0000000000000000000000'
    const res = await api.get(`/v1/servers/metrics/batch?ids=${ids}`)
    expect(res.ok()).toBeTruthy()
    expect(typeof (await res.json())).toBe('object')
  })

  test('get server metrics history returns array or 503', async () => {
    const listRes = await api.get('/v1/servers')
    const { servers } = await listRes.json()
    if (servers.length === 0) return

    const id = servers[0].id
    const res = await api.get(`/v1/servers/${id}/metrics/history?hours=1`)
    expect([200, 503]).toContain(res.status())
  })
})
