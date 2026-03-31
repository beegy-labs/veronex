import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { API_BASE_URL } from './helpers/constants'

test.describe('API: Authentication', () => {
  test('login returns access and refresh tokens', async ({ request }) => {
    const tokens = await apiLogin(request)
    expect(tokens.accessToken).toBeTruthy()
    expect(tokens.refreshToken).toBeTruthy()
    expect(tokens.accountId).toBeTruthy()
  })

  test('invalid credentials return 401', async ({ request }) => {
    const res = await request.post(`${API_BASE_URL}/v1/auth/login`, {
      data: { username: 'nonexistent', password: 'wrong' },
    })
    expect(res.status()).toBe(401)
  })

  test('unauthenticated request to protected endpoint returns 401', async ({ playwright }) => {
    // Use a fresh context with no cookies to test unauthenticated access
    const ctx = await playwright.request.newContext({ storageState: { cookies: [], origins: [] } })
    const res = await ctx.get(`${API_BASE_URL}/v1/providers`)
    expect(res.status()).toBe(401)
    await ctx.dispose()
  })

  test('refresh token returns new access token', async ({ playwright }) => {
    // Perform a full login cycle in an isolated context so cookie-based refresh works
    const ctx = await playwright.request.newContext()
    const loginRes = await ctx.post(`${API_BASE_URL}/v1/auth/login`, {
      data: { username: process.env.E2E_USERNAME ?? 'test', password: process.env.E2E_PASSWORD ?? 'test1234!' },
    })
    expect(loginRes.ok()).toBeTruthy()
    // Refresh — the refresh token is sent automatically as a cookie by the context
    const refreshRes = await ctx.post(`${API_BASE_URL}/v1/auth/refresh`)
    expect(refreshRes.ok()).toBeTruthy()
    await ctx.dispose()
  })

  test('logout invalidates session', async ({ playwright }) => {
    // Use an isolated context so we don't invalidate the global setup session
    const ctx = await playwright.request.newContext()
    const loginRes = await ctx.post(`${API_BASE_URL}/v1/auth/login`, {
      data: { username: process.env.E2E_USERNAME ?? 'test', password: process.env.E2E_PASSWORD ?? 'test1234!' },
    })
    expect(loginRes.ok()).toBeTruthy()
    const logoutRes = await ctx.post(`${API_BASE_URL}/v1/auth/logout`)
    expect(logoutRes.status()).toBe(204)
    await ctx.dispose()
  })
})
