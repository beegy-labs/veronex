/**
 * ApiClient — HTTP transport layer for JWT-protected API calls.
 *
 * Auth tokens are HttpOnly cookies — the browser sends them automatically
 * when `credentials: 'include'` is set.  No Authorization header needed.
 *
 * Auth flow (refresh mutex, redirect) is owned by auth-guard.ts — not here.
 * This module only handles: fetch with credentials → handle 401 via auth-guard.
 */

import { tryRefresh, redirectToLogin } from './auth-guard'
import { BASE_API_URL as BASE } from './constants'

class ApiClient {
  private async fetchWithCredentials(path: string, init?: RequestInit): Promise<Response> {
    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 30_000)
    try {
      return await fetch(`${BASE}${path}`, {
        ...init,
        headers: {
          'Content-Type': 'application/json',
          ...init?.headers,
        },
        credentials: 'include',
        signal: controller.signal,
        cache: 'no-store',
      })
    } finally {
      clearTimeout(timeoutId)
    }
  }

  async request<T>(path: string, init?: RequestInit): Promise<T> {
    let res = await this.fetchWithCredentials(path, init)

    if (res.status === 401) {
      const refreshed = await tryRefresh()
      if (!refreshed) {
        redirectToLogin()
        throw new Error('Unauthorized')
      }
      res = await this.fetchWithCredentials(path, init)
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
