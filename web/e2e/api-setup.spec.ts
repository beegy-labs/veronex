import { test, expect } from '@playwright/test'
import { API_BASE_URL } from './helpers/constants'

test.describe('API: Setup', () => {
  // Setup status is a public endpoint — no auth required.

  test('setup status returns needs_setup boolean', async ({ request }) => {
    const res = await request.get(`${API_BASE_URL}/v1/setup/status`)
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(typeof body.needs_setup).toBe('boolean')
    // Since E2E tests run against a seeded server, setup should be complete.
    expect(body.needs_setup).toBe(false)
  })
})
