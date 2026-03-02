/**
 * AuthGuard — SSOT for JWT token lifecycle.
 *
 * Owns:
 *   PUBLIC_PATHS     — pages that suppress redirect-to-login
 *   tryRefresh()     — token refresh with Promise mutex (deduplicates concurrent 401s)
 *   redirectToLogin()— clear tokens + navigate; no-op when already on a public path
 *
 * api-client.ts delegates here.
 * nav.tsx logout should call redirectToLogin() instead of rolling its own.
 * Never duplicate refresh or redirect logic elsewhere.
 */

import { clearTokens, getRefreshToken, setAccessToken } from './auth'

const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

// ── Public paths ───────────────────────────────────────────────────────────────
// Add any route that is accessible without authentication.
// Redirect-to-login is suppressed when the current pathname matches.

export const PUBLIC_PATHS = ['/login', '/setup'] as const
export type PublicPath = (typeof PUBLIC_PATHS)[number]

export function isPublicPath(pathname: string): boolean {
  return (PUBLIC_PATHS as readonly string[]).includes(pathname)
}

// ── Refresh mutex ──────────────────────────────────────────────────────────────
// Module-level — survives component re-renders, reset only on full page reload.
// All concurrent 401 handlers share the same in-flight Promise.

let refreshMutex: Promise<boolean> | null = null

async function doRefresh(): Promise<boolean> {
  const rt = getRefreshToken()
  if (!rt) return false
  try {
    const r = await fetch(`${BASE}/v1/auth/refresh`, {
      method: 'POST',
      body: JSON.stringify({ refresh_token: rt }),
      headers: { 'Content-Type': 'application/json' },
      cache: 'no-store',
    })
    if (!r.ok) return false
    const { access_token } = await r.json()
    setAccessToken(access_token)
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
 * Redirect to /login after clearing all auth cookies.
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
  clearTokens()
  window.location.href = '/login'
}
