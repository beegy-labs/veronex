import type { LoginResponse } from './types'

const ACCESS_TOKEN_KEY = 'veronex_access_token'
const REFRESH_TOKEN_KEY = 'veronex_refresh_token'
const USERNAME_KEY = 'veronex_username'
const ROLE_KEY = 'veronex_role'
const ACCOUNT_ID_KEY = 'veronex_account_id'

export function getAccessToken(): string | null {
  if (typeof window === 'undefined') return null
  return localStorage.getItem(ACCESS_TOKEN_KEY)
}

export function getRefreshToken(): string | null {
  if (typeof window === 'undefined') return null
  return localStorage.getItem(REFRESH_TOKEN_KEY)
}

export function setTokens(resp: LoginResponse): void {
  localStorage.setItem(ACCESS_TOKEN_KEY, resp.access_token)
  localStorage.setItem(USERNAME_KEY, resp.username)
  localStorage.setItem(ROLE_KEY, resp.role)
  localStorage.setItem(ACCOUNT_ID_KEY, resp.account_id)
  if (resp.refresh_token) {
    localStorage.setItem(REFRESH_TOKEN_KEY, resp.refresh_token)
  }
}

export function setAccessToken(token: string): void {
  if (typeof window === 'undefined') return
  localStorage.setItem(ACCESS_TOKEN_KEY, token)
}

export function clearTokens(): void {
  localStorage.removeItem(ACCESS_TOKEN_KEY)
  localStorage.removeItem(REFRESH_TOKEN_KEY)
  localStorage.removeItem(USERNAME_KEY)
  localStorage.removeItem(ROLE_KEY)
  localStorage.removeItem(ACCOUNT_ID_KEY)
}

export function getAuthUser(): { username: string; role: string; accountId: string } | null {
  if (typeof window === 'undefined') return null
  const token = getAccessToken()
  if (!token) return null
  const username = localStorage.getItem(USERNAME_KEY)
  const role = localStorage.getItem(ROLE_KEY)
  const accountId = localStorage.getItem(ACCOUNT_ID_KEY)
  if (!username || !role || !accountId) return null
  return { username, role, accountId }
}

export function isLoggedIn(): boolean {
  return !!getAccessToken()
}
