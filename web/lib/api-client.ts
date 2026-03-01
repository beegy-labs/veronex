/**
 * ApiClient — SSOT for all JWT-protected API calls.
 *
 * Automatically attaches `Authorization: Bearer <access_token>`.
 * On 401: attempts a single token refresh, then retries the request.
 * If refresh fails, clears all stored tokens and redirects to /login.
 */

import { clearTokens, getAccessToken, getRefreshToken, setAccessToken } from './auth'

const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

class ApiClient {
  private async fetchWithToken(path: string, init?: RequestInit): Promise<Response> {
    const token = getAccessToken()
    return fetch(`${BASE}${path}`, {
      ...init,
      headers: {
        Authorization: token ? `Bearer ${token}` : '',
        'Content-Type': 'application/json',
        ...init?.headers,
      },
      cache: 'no-store',
    })
  }

  private async tryRefresh(): Promise<boolean> {
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

  async request<T>(path: string, init?: RequestInit): Promise<T> {
    let res = await this.fetchWithToken(path, init)

    if (res.status === 401) {
      const refreshed = await this.tryRefresh()
      if (!refreshed) {
        clearTokens()
        if (typeof window !== 'undefined') {
          window.location.href = '/login'
        }
        throw new Error('Unauthorized')
      }
      res = await this.fetchWithToken(path, init)
    }

    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
    if (res.status === 204) return undefined as T
    return res.json() as Promise<T>
  }

  get<T>(path: string): Promise<T> {
    return this.request<T>(path)
  }

  post<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>(path, {
      method: 'POST',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    })
  }

  patch<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>(path, {
      method: 'PATCH',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    })
  }

  put<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>(path, {
      method: 'PUT',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    })
  }

  delete<T>(path: string): Promise<T> {
    return this.request<T>(path, { method: 'DELETE' })
  }
}

export const apiClient = new ApiClient()
