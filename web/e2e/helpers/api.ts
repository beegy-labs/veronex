import { APIRequestContext } from '@playwright/test'
import { API_BASE_URL, TEST_USERNAME, TEST_PASSWORD } from './constants'

export interface AuthTokens {
  accessToken: string
  refreshToken: string
  accountId: string
}

/**
 * Authenticate via the REST API and return JWT tokens.
 */
export async function apiLogin(request: APIRequestContext): Promise<AuthTokens> {
  const res = await request.post(`${API_BASE_URL}/v1/auth/login`, {
    data: {
      username: TEST_USERNAME,
      password: TEST_PASSWORD,
    },
  })
  if (!res.ok()) throw new Error(`Login failed: ${res.status()}`)
  const body = await res.json()
  return {
    accessToken: body.access_token,
    refreshToken: body.refresh_token,
    accountId: body.account_id,
  }
}

/**
 * Create an authenticated request helper with auto-attached Bearer token.
 */
export function authedRequest(request: APIRequestContext, token: string) {
  const headers = { Authorization: `Bearer ${token}` }

  return {
    get: (path: string) =>
      request.get(`${API_BASE_URL}${path}`, { headers }),
    post: (path: string, data?: unknown) =>
      request.post(`${API_BASE_URL}${path}`, { headers, data }),
    patch: (path: string, data?: unknown) =>
      request.patch(`${API_BASE_URL}${path}`, { headers, data }),
    put: (path: string, data?: unknown) =>
      request.put(`${API_BASE_URL}${path}`, { headers, data }),
    delete: (path: string) =>
      request.delete(`${API_BASE_URL}${path}`, { headers }),
  }
}
