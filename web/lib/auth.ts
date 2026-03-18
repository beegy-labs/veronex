import type { LoginResponse } from './types'

const SESSION_KEY      = 'veronex_session'
const USERNAME_KEY     = 'veronex_username'
const ROLE_KEY         = 'veronex_role'
const ACCOUNT_ID_KEY   = 'veronex_account_id'
const PERMISSIONS_KEY  = 'veronex_permissions'
const MENUS_KEY        = 'veronex_menus'

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

/**
 * Store non-sensitive session indicator cookies after a successful login/setup.
 *
 * Auth tokens (access_token, refresh_token) are now set as HttpOnly cookies
 * by the backend — they are never accessible to JavaScript.
 */
export function setSession(resp: LoginResponse): void {
  writeCookie(SESSION_KEY,     '1')
  writeCookie(USERNAME_KEY,    resp.username)
  writeCookie(ROLE_KEY,        resp.role)
  writeCookie(ACCOUNT_ID_KEY,  resp.account_id)
  writeCookie(PERMISSIONS_KEY, JSON.stringify(resp.permissions ?? []))
  writeCookie(MENUS_KEY,       JSON.stringify(resp.menus ?? []))
}

/**
 * Clear all client-side session indicator cookies.
 *
 * NOTE: The HttpOnly auth cookies (access_token, refresh_token) are cleared
 * by the backend via Set-Cookie headers on the logout response.
 */
export function clearSession(): void {
  deleteCookie(SESSION_KEY)
  deleteCookie(USERNAME_KEY)
  deleteCookie(ROLE_KEY)
  deleteCookie(ACCOUNT_ID_KEY)
  deleteCookie(PERMISSIONS_KEY)
  deleteCookie(MENUS_KEY)
}

export function getAuthUser(): {
  username: string
  role: string
  accountId: string
  permissions: string[]
  menus: string[]
} | null {
  if (!isLoggedIn()) return null
  const username  = readCookie(USERNAME_KEY)
  const role      = readCookie(ROLE_KEY)
  const accountId = readCookie(ACCOUNT_ID_KEY)
  if (!username || !role || !accountId) return null

  let permissions: string[] = []
  let menus: string[] = []
  try {
    permissions = JSON.parse(readCookie(PERMISSIONS_KEY) ?? '[]')
  } catch { /* empty */ }
  try {
    menus = JSON.parse(readCookie(MENUS_KEY) ?? '[]')
  } catch { /* empty */ }

  return { username, role, accountId, permissions, menus }
}

/**
 * Check whether the user has an active session indicator.
 *
 * This reads the non-HttpOnly `veronex_session` cookie which is set by JS
 * on login success.  The actual auth tokens are HttpOnly and cannot be read
 * by JavaScript — the browser sends them automatically.
 */
export function isLoggedIn(): boolean {
  return readCookie(SESSION_KEY) === '1'
}

/** Check if the current user has a specific permission. Super role has all permissions. */
export function hasPermission(perm: string): boolean {
  const user = getAuthUser()
  if (!user) return false
  if (user.role === 'super') return true
  return user.permissions.includes(perm)
}

/** Check if the current user has access to a specific menu. Super role has all menus. */
export function hasMenu(menuId: string): boolean {
  const user = getAuthUser()
  if (!user) return false
  if (user.role === 'super') return true
  return user.menus.includes(menuId)
}
