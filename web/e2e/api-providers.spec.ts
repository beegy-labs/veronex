import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { testId } from './helpers/constants'

test.describe('API: Providers', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list providers returns array', async () => {
    const res = await api.get('/v1/providers')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBeTruthy()
  })

  test('register provider requires provider_type', async () => {
    const res = await api.post('/v1/providers', {
      name: 'bad-provider',
      provider_type: 'invalid',
    })
    expect(res.status()).toBe(400)
  })

  test('register ollama provider requires url', async () => {
    const res = await api.post('/v1/providers', {
      name: 'no-url-ollama',
      provider_type: 'ollama',
    })
    expect(res.status()).toBe(400)
    const body = await res.json()
    expect(body.error).toContain('url')
  })

  test('register gemini provider requires api_key', async () => {
    const res = await api.post('/v1/providers', {
      name: 'no-key-gemini',
      provider_type: 'gemini',
    })
    expect(res.status()).toBe(400)
    const body = await res.json()
    expect(body.error).toContain('api_key')
  })

  test('provider CRUD lifecycle (ollama)', async () => {
    let providerId: string | undefined
    try {
      // Register (will likely be offline since no real Ollama is running)
      const createRes = await api.post('/v1/providers', {
        name: `e2e-ollama-${testId()}`,
        provider_type: 'ollama',
        url: 'http://127.0.0.1:99999', // non-existent — health check will set offline
      })
      expect(createRes.status()).toBe(201)
      const { id, status } = await createRes.json()
      providerId = id
      expect(id).toBeTruthy()
      expect(status).toBe('offline') // can't reach the fake URL

      // Update name
      const updateRes = await api.patch(`/v1/providers/${id}`, {
        name: 'e2e-ollama-updated',
      })
      expect(updateRes.ok()).toBeTruthy()
      const updated = await updateRes.json()
      expect(updated.name).toBe('e2e-ollama-updated')

      // Delete
      const deleteRes = await api.delete(`/v1/providers/${id}`)
      expect(deleteRes.status()).toBe(204)
    } finally {
      if (providerId) await api.delete(`/v1/providers/${providerId}`)
    }
  })

  test('healthcheck on non-existent provider returns 404', async () => {
    const res = await api.post('/v1/providers/00000000-0000-0000-0000-000000000000/healthcheck')
    expect(res.status()).toBe(404)
  })

  test('get provider models on non-existent provider returns 404', async () => {
    const res = await api.get('/v1/providers/00000000-0000-0000-0000-000000000000/models')
    expect(res.status()).toBe(404)
  })
})
