import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { API_BASE_URL, testId } from './helpers/constants'
import { APIRequestContext } from '@playwright/test'

test.describe('API: OpenAI Compat Endpoints', () => {
  let mgmt: ReturnType<typeof authedRequest>
  let accountId: string
  let apiKeyValue: string
  let apiKeyId: string

  test.beforeAll(async ({ playwright }) => {
    const request: APIRequestContext = await playwright.request.newContext()
    const tokens = await apiLogin(request)
    mgmt = authedRequest(request, tokens.accessToken)
    accountId = tokens.accountId

    // Create a temporary API key for OpenAI compat tests
    const res = await mgmt.post('/v1/keys', {
      tenant_id: accountId,
      name: `e2e-compat-${testId()}`,
      tier: 'free',
    })
    if (res.ok()) {
      const body = await res.json()
      apiKeyId = body.id
      apiKeyValue = body.key
    }
  })

  test.afterAll(async () => {
    if (apiKeyId) await mgmt.delete(`/v1/keys/${apiKeyId}`)
  })

  function apiKeyRequest(request: APIRequestContext) {
    const headers = { Authorization: `Bearer ${apiKeyValue}` }
    return {
      get: (path: string) => request.get(`${API_BASE_URL}${path}`, { headers }),
      post: (path: string, data?: unknown) =>
        request.post(`${API_BASE_URL}${path}`, { headers, data }),
    }
  }

  // ── /v1/models ──────────────────────────────────────────────────────────────
  test('list models returns OpenAI-compatible response', async ({ request }) => {
    if (!apiKeyValue) return
    const api = apiKeyRequest(request)
    const res = await api.get('/v1/models')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(body.object).toBe('list')
    expect(Array.isArray(body.data)).toBeTruthy()
  })

  test('get model by id returns model or 404', async ({ request }) => {
    if (!apiKeyValue) return
    const api = apiKeyRequest(request)
    const listRes = await api.get('/v1/models')
    const body = await listRes.json()
    if (body.data.length === 0) return

    const modelId = body.data[0].id
    const res = await api.get(`/v1/models/${encodeURIComponent(modelId)}`)
    expect([200, 404]).toContain(res.status())
    if (res.status() === 200) {
      const model = await res.json()
      expect(model.object).toBe('model')
      expect(typeof model.id).toBe('string')
    }
  })

  // ── /v1/completions ─────────────────────────────────────────────────────────
  test('text completions returns 200 or 503 when no provider available', async ({ request }) => {
    if (!apiKeyValue) return
    const api = apiKeyRequest(request)
    const listRes = await api.get('/v1/models')
    const { data } = await listRes.json()
    if (data.length === 0) return

    const res = await api.post('/v1/completions', {
      model: data[0].id,
      prompt: 'Hello',
      max_tokens: 8,
      stream: false,
    })
    expect([200, 503]).toContain(res.status())
  })

  // ── /v1/embeddings ──────────────────────────────────────────────────────────
  test('embeddings returns 200, 503, or 400 depending on provider', async ({ request }) => {
    if (!apiKeyValue) return
    const api = apiKeyRequest(request)
    const listRes = await api.get('/v1/models')
    const { data } = await listRes.json()
    if (data.length === 0) return

    const res = await api.post('/v1/embeddings', {
      model: data[0].id,
      input: 'test embedding',
    })
    // 200 if embedding model available, 503/502 if no provider, 400/422 if model doesn't support it
    expect([200, 502, 503, 422, 400]).toContain(res.status())
  })
})
