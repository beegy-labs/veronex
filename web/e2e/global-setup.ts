import { request } from '@playwright/test'
import * as fs from 'fs'
import * as path from 'path'
import { TEST_USERNAME, TEST_PASSWORD, API_BASE_URL } from './helpers/constants'

const AUTH_FILE = path.join(__dirname, '.auth.json')
const API_TOKEN_FILE = path.join(__dirname, '.api-token.json')

/**
 * Global setup: authenticate once via API and build storageState from response cookies.
 * Avoids browser-based login which can be blocked by IP rate limits or autofill interference.
 */
export default async function globalSetup() {
  const ctx = await request.newContext({ baseURL: API_BASE_URL })

  // Login via API
  const res = await ctx.post('/v1/auth/login', {
    data: { username: TEST_USERNAME, password: TEST_PASSWORD },
  })
  if (!res.ok()) throw new Error(`Login failed: ${res.status()} ${await res.text()}`)

  const body = await res.json()

  // Tokens are returned as HttpOnly cookies, not in body — extract from Set-Cookie headers
  const setCookieHeaders = res.headersArray().filter(h => h.name.toLowerCase() === 'set-cookie')
  const accessTokenCookie = setCookieHeaders.find(h => h.value.startsWith('veronex_access_token='))
  const refreshTokenCookie = setCookieHeaders.find(h => h.value.startsWith('veronex_refresh_token='))
  const accessToken = accessTokenCookie?.value.split(';')[0].split('=').slice(1).join('=') ?? ''
  const refreshToken = refreshTokenCookie?.value.split(';')[0].split('=').slice(1).join('=') ?? ''

  // Save API token for api-*.spec.ts tests
  fs.writeFileSync(API_TOKEN_FILE, JSON.stringify({
    accessToken,
    refreshToken,
    accountId: body.account_id,
  }))

  // Build storageState from API response cookies
  // The login response sets HttpOnly cookies (access_token, refresh_token)
  const httpOnlyCookies = setCookieHeaders.map(h => parseSetCookie(h.value, 'localhost', '/'))

  // Add JS-readable session indicator cookies (set by auth.ts setSession() after login)
  // The Next.js middleware (proxy.ts) checks veronex_session to gate protected routes
  const expires = Date.now() / 1000 + 7 * 86400
  const sessionCookies = [
    { name: 'veronex_session',     value: '1' },
    { name: 'veronex_username',    value: encodeURIComponent(body.username) },
    { name: 'veronex_role',        value: encodeURIComponent(body.role) },
    { name: 'veronex_account_id',  value: encodeURIComponent(body.account_id) },
    { name: 'veronex_permissions', value: encodeURIComponent(JSON.stringify(body.permissions ?? [])) },
    { name: 'veronex_menus',       value: encodeURIComponent(JSON.stringify(body.menus ?? [])) },
  ].map(c => ({ ...c, domain: 'localhost', path: '/', expires, httpOnly: false, secure: false, sameSite: 'Strict' as const }))

  fs.writeFileSync(AUTH_FILE, JSON.stringify({
    cookies: [...httpOnlyCookies, ...sessionCookies],
    origins: [{ origin: 'http://localhost:3002', localStorage: [] }],
  }))

  console.log(`[global-setup] Logged in as "${TEST_USERNAME}", saved ${httpOnlyCookies.length + sessionCookies.length} cookies`)
  await ctx.dispose()
}

function parseSetCookie(raw: string, domain: string, path: string) {
  const parts = raw.split(';').map(p => p.trim())
  const [nameValue, ...attrs] = parts
  const eqIdx = nameValue.indexOf('=')
  const name = nameValue.slice(0, eqIdx)
  const value = nameValue.slice(eqIdx + 1)

  const attrMap: Record<string, string> = {}
  for (const attr of attrs) {
    const [k, v] = attr.split('=').map(s => s.trim())
    attrMap[k.toLowerCase()] = v ?? ''
  }

  return {
    name,
    value,
    domain: attrMap['domain'] ?? domain,
    path: attrMap['path'] ?? path,
    expires: attrMap['max-age']
      ? Date.now() / 1000 + parseInt(attrMap['max-age'])
      : attrMap['expires']
        ? new Date(attrMap['expires']).getTime() / 1000
        : -1,
    httpOnly: 'httponly' in attrMap,
    // Force secure:false — tests run over http://localhost, secure cookies are not sent over HTTP
    secure: false,
    sameSite: 'Lax' as const,
  }
}
