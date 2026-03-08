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

  test('unauthenticated request to protected endpoint returns 401', async ({ request }) => {
    const res = await request.get(`${API_BASE_URL}/v1/providers`)
    expect(res.status()).toBe(401)
  })

  test('refresh token returns new access token', async ({ request }) => {
    const tokens = await apiLogin(request)
    const res = await request.post(`${API_BASE_URL}/v1/auth/refresh`, {
      data: { refresh_token: tokens.refreshToken },
    })
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(body.access_token).toBeTruthy()
    expect(body.token_type).toBe('Bearer')
  })

  test('logout invalidates session', async ({ request }) => {
    const tokens = await apiLogin(request)
    const res = await request.post(`${API_BASE_URL}/v1/auth/logout`, {
      data: { refresh_token: tokens.refreshToken },
    })
    expect(res.status()).toBe(204)
  })
})
