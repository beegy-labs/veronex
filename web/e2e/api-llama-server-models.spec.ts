import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'

test.describe('API: llama-server Models', () => {
  let api: ReturnType<typeof authedRequest>

  test.beforeEach(async ({ request }) => {
    const tokens = await apiLogin(request)
    api = authedRequest(request, tokens.accessToken)
  })

  test('list llama_server models returns models array', async () => {
    const res = await api.get('/v1/llama_server/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body.models)).toBeTruthy()
  })

  test('llama_server model entries have expected shape', async () => {
    const res = await api.get('/v1/llama_server/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()

    if (body.models.length > 0) {
      const model = body.models[0]
      expect(typeof model.model_name).toBe('string')
      expect(typeof model.provider_count).toBe('number')
    }
  })

  test('list model providers returns paginated result', async () => {
    const res = await api.get('/v1/llama_server/models')
    const body = await res.json()
    if (body.models.length === 0) return

    const modelName = body.models[0].model_name
    const provRes = await api.get(`/v1/llama_server/models/${encodeURIComponent(modelName)}/providers`)
    expect(provRes.ok()).toBeTruthy()
    const provBody = await provRes.json()
    expect(Array.isArray(provBody.providers ?? provBody)).toBeTruthy()
  })

  test('sync status returns status or 404 when no sync has run', async () => {
    const res = await api.get('/v1/llama_server/sync/status')
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
