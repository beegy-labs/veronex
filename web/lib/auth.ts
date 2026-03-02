import type { LoginResponse } from './types'

const ACCESS_TOKEN_KEY  = 'veronex_access_token'
const REFRESH_TOKEN_KEY = 'veronex_refresh_token'
const USERNAME_KEY      = 'veronex_username'
const ROLE_KEY          = 'veronex_role'
const ACCOUNT_ID_KEY    = 'veronex_account_id'

// ── Cookie helpers (same pattern as timezone-provider.tsx) ─────────────────

function readCookie(name: string): string | null {
  if (typeof document === 'undefined') return null
  const match = document.cookie.match(new RegExp(`(?:^|;\\s*)${name}=([^;]*)`))
  return match ? decodeURIComponent(match[1]) : null
}

function writeCookie(name: string, value: string, days = 7): void {
  const expires = new Date(Date.now() + days * 864e5).toUTCString()
  document.cookie = `${name}=${encodeURIComponent(value)}; path=/; expires=${expires}; SameSite=Strict`
}

function deleteCookie(name: string): void {
  document.cookie = `${name}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Strict`
}

// ── Public API ─────────────────────────────────────────────────────────────

export function getAccessToken(): string | null {
  return readCookie(ACCESS_TOKEN_KEY)
}

export function getRefreshToken(): string | null {
  return readCookie(REFRESH_TOKEN_KEY)
}

export function setTokens(resp: LoginResponse): void {
  writeCookie(ACCESS_TOKEN_KEY, resp.access_token)
  writeCookie(USERNAME_KEY,     resp.username)
  writeCookie(ROLE_KEY,         resp.role)
  writeCookie(ACCOUNT_ID_KEY,   resp.account_id)
  if (resp.refresh_token) {
    writeCookie(REFRESH_TOKEN_KEY, resp.refresh_token)
  }
}

export function setAccessToken(token: string): void {
  writeCookie(ACCESS_TOKEN_KEY, token)
}

export function clearTokens(): void {
  deleteCookie(ACCESS_TOKEN_KEY)
  deleteCookie(REFRESH_TOKEN_KEY)
  deleteCookie(USERNAME_KEY)
  deleteCookie(ROLE_KEY)
  deleteCookie(ACCOUNT_ID_KEY)
}

export function getAuthUser(): { username: string; role: string; accountId: string } | null {
  const token = getAccessToken()
  if (!token) return null
  const username  = readCookie(USERNAME_KEY)
  const role      = readCookie(ROLE_KEY)
  const accountId = readCookie(ACCOUNT_ID_KEY)
  if (!username || !role || !accountId) return null
  return { username, role, accountId }
}

export function isLoggedIn(): boolean {
  return !!getAccessToken()
}
