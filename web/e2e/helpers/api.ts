import { APIRequestContext } from '@playwright/test'
import * as fs from 'fs'
import * as path from 'path'
import { API_BASE_URL, TEST_USERNAME, TEST_PASSWORD } from './constants'

export interface AuthTokens {
  accessToken: string
  refreshToken: string
  accountId: string
}

const API_TOKEN_FILE = path.join(__dirname, '../.api-token.json')

/**
 * Return cached API tokens from global setup.
 * Falls back to a real login if the cache file is missing (e.g. solo spec run).
 */
export async function apiLogin(request: APIRequestContext): Promise<AuthTokens> {
  if (fs.existsSync(API_TOKEN_FILE)) {
    return JSON.parse(fs.readFileSync(API_TOKEN_FILE, 'utf-8')) as AuthTokens
  }
  // Fallback: real login (may hit rate limit if called many times)
  const res = await request.post(`${API_BASE_URL}/v1/auth/login`, {
    data: { username: TEST_USERNAME, password: TEST_PASSWORD },
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
