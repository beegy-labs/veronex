/**
 * AuthGuard — SSOT for JWT token lifecycle.
 *
 * Owns:
 *   PUBLIC_PATHS     — pages that suppress redirect-to-login
 *   tryRefresh()     — token refresh via HttpOnly cookie (deduplicates concurrent 401s)
 *   redirectToLogin()— clear session + navigate; no-op when already on a public path
 *
 * api-client.ts delegates here.
 * nav.tsx logout should call redirectToLogin() instead of rolling its own.
 * Never duplicate refresh or redirect logic elsewhere.
 *
 * Auth tokens are HttpOnly — JavaScript never touches them.
 * The browser sends them automatically via credentials: 'include'.
 */

import { clearSession } from './auth'
import { BASE_API_URL as BASE } from './constants'

// ── Public paths ───────────────────────────────────────────────────────────────
// Add any route that is accessible without authentication.
// Redirect-to-login is suppressed when the current pathname matches.

export const PUBLIC_PATHS = ['/login', '/setup'] as const

export function isPublicPath(pathname: string): boolean {
  return (PUBLIC_PATHS as readonly string[]).includes(pathname)
}

// ── Refresh mutex ──────────────────────────────────────────────────────────────
// Module-level — survives component re-renders, reset only on full page reload.
// All concurrent 401 handlers share the same in-flight Promise.

let refreshMutex: Promise<boolean> | null = null

async function doRefresh(): Promise<boolean> {
  try {
    // The refresh token is in an HttpOnly cookie scoped to /v1/auth —
    // the browser sends it automatically when credentials: 'include' is set.
    const r = await fetch(`${BASE}/v1/auth/refresh`, {
      method: 'POST',
      credentials: 'include',
      cache: 'no-store',
    })
    if (!r.ok) return false
    // The new access token is set as an HttpOnly cookie via Set-Cookie header.
    // Nothing to read from the response body.
    return true
  } catch {
    return false
  }
}

/**
 * Attempt token refresh, deduplicated across concurrent callers.
 *
 * When N requests get 401 simultaneously:
 *   - First caller: creates the refresh Promise (sets refreshMutex)
 *   - Others: return the same Promise (piggyback)
 *   - All receive the same boolean result and retry with the new token
 */
export function tryRefresh(): Promise<boolean> {
  if (refreshMutex !== null) return refreshMutex
  refreshMutex = doRefresh().finally(() => { refreshMutex = null })
  return refreshMutex
}

// ── Redirect guard ─────────────────────────────────────────────────────────────

let redirecting = false

/**
 * Redirect to /login after clearing all session cookies.
 *
 * No-op when:
 *   - Already redirecting (prevents duplicate full-page reloads)
 *   - Current path is a public path (prevents reload loops on /login and /setup)
 */
export function redirectToLogin(): void {
  if (redirecting) return
  if (typeof window === 'undefined') return
  if (isPublicPath(window.location.pathname)) return
  redirecting = true
  clearSession()
  window.location.href = '/login'
}
