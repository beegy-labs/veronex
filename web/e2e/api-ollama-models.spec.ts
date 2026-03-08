import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: Ollama Models', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list ollama models returns models array', async () => {
    const res = await api.get('/v1/ollama/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.models)).toBeTruthy()
  })

  test('ollama model entries have expected shape', async () => {
    const res = await api.get('/v1/ollama/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()

    if (body.models.length > 0) {
      const model = body.models[0]
      expect(typeof model.model_name).toBe('string')
      expect(typeof model.provider_count).toBe('number')
    }
  })

  test('sync status returns status or 404 when no sync has run', async () => {
    const res = await api.get('/v1/ollama/sync/status')
    // 200 if a sync job exists, 404 if no sync has ever run
    expect([200, 404]).toContain(res.status())

    if (res.status() === 200) {
      const body = await res.json()
      expect(typeof body.id).toBe('string')
      expect(typeof body.status).toBe('string')
      expect(typeof body.total_providers).toBe('number')
      expect(typeof body.done_providers).toBe('number')
    }
  })
})
