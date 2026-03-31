import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: MCP Servers', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list mcp servers returns array', async () => {
    const res = await api.get('/v1/mcp/servers')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  test('mcp server CRUD lifecycle', async () => {
    const name = `e2e-mcp-${testId()}`
    const slug = `e2e${testId().replace(/-/g, '')}`
    let serverId: string | undefined
    try {
      // Register
      const createRes = await api.post('/v1/mcp/servers', {
        name,
        slug,
        url: 'http://localhost:3100',
        timeout_secs: 30,
      })
      expect(createRes.status()).toBe(201)
      const { id } = await createRes.json()
      serverId = id
      expect(id).toBeTruthy()

      // List — server appears
      const listRes = await api.get('/v1/mcp/servers')
      const servers = await listRes.json()
      const found = servers.find((s: { id: string }) => s.id === id)
      expect(found).toBeTruthy()
      expect(found.name).toBe(name)
      expect(found.slug).toBe(slug)
      expect(typeof found.is_enabled).toBe('boolean')
      expect(typeof found.online).toBe('boolean')
      expect(typeof found.tool_count).toBe('number')

      // Patch — toggle enabled
      const patchRes = await api.patch(`/v1/mcp/servers/${id}`, { is_enabled: false })
      expect(patchRes.ok()).toBeTruthy()
      const patched = await patchRes.json()
      expect(patched.is_enabled).toBe(false)

      // Delete
      const deleteRes = await api.delete(`/v1/mcp/servers/${id}`)
      expect([200, 204]).toContain(deleteRes.status())
      serverId = undefined

      // Verify deleted
      const listRes2 = await api.get('/v1/mcp/servers')
      const servers2 = await listRes2.json()
      expect(servers2.find((s: { id: string }) => s.id === id)).toBeUndefined()
    } finally {
      if (serverId) await api.delete(`/v1/mcp/servers/${serverId}`)
    }
  })

  test('register with empty name returns 400', async () => {
    const res = await api.post('/v1/mcp/servers', {
      name: '',
      slug: 'valid-slug',
      url: 'http://localhost:3100',
    })
    expect(res.status()).toBe(400)
  })

  test('register with invalid slug returns 400', async () => {
    const res = await api.post('/v1/mcp/servers', {
      name: 'Valid Name',
      slug: 'Invalid-Slug', // uppercase not allowed
      url: 'http://localhost:3100',
    })
    expect(res.status()).toBe(400)
  })

  test('patch non-existent server returns 404', async () => {
    const res = await api.patch('/v1/mcp/servers/00000000-0000-0000-0000-000000000000', {
      is_enabled: true,
    })
    expect(res.status()).toBe(404)
  })

  test('delete non-existent server returns 404', async () => {
    const res = await api.delete('/v1/mcp/servers/00000000-0000-0000-0000-000000000000')
    expect(res.status()).toBe(404)
  })
})
