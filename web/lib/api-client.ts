/**
 * ApiClient — HTTP transport layer for JWT-protected API calls.
 *
 * Auth flow (refresh mutex, redirect) is owned by auth-guard.ts — not here.
 * This module only handles: attach token → fetch → handle 401 via auth-guard.
 */

import { getAccessToken } from './auth'
import { tryRefresh, redirectToLogin } from './auth-guard'

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

  async request<T>(path: string, init?: RequestInit): Promise<T> {
    let res = await this.fetchWithToken(path, init)

    if (res.status === 401) {
      const refreshed = await tryRefresh()
      if (!refreshed) {
        redirectToLogin()
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
