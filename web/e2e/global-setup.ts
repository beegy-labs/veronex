import { chromium, request } from '@playwright/test'
import * as fs from 'fs'
import * as path from 'path'
import { TEST_USERNAME, TEST_PASSWORD, API_BASE_URL } from './helpers/constants'

const AUTH_FILE = path.join(__dirname, '.auth.json')
const API_TOKEN_FILE = path.join(__dirname, '.api-token.json')

/**
 * Global setup: authenticate once (browser + API) and save state.
 * All tests reuse this — avoids hitting the IP-based login rate limit
 * (10 attempts per 5 minutes per IP).
 */
export default async function globalSetup() {
  // ── Browser session ─────────────────────────────────────────────────────────
  const browser = await chromium.launch()
  const page = await browser.newPage()
  const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? 'http://localhost:3002'

  await page.goto(`${baseURL}/login`)
  await page.locator('#username').fill(TEST_USERNAME)
  await page.locator('#password').fill(TEST_PASSWORD)
  await page.getByRole('button', { name: /sign in/i }).click()
  await page.waitForURL(/\/(overview|setup|dashboard)/, { timeout: 30_000 })
  await page.context().storageState({ path: AUTH_FILE })
  await browser.close()

  // ── API token (for api-*.spec.ts) ────────────────────────────────────────────
  const ctx = await request.newContext()
  const res = await ctx.post(`${API_BASE_URL}/v1/auth/login`, {
    data: { username: TEST_USERNAME, password: TEST_PASSWORD },
  })
  if (!res.ok()) throw new Error(`API login failed: ${res.status()}`)
  const body = await res.json()
  fs.writeFileSync(API_TOKEN_FILE, JSON.stringify({
    accessToken: body.access_token,
    refreshToken: body.refresh_token,
    accountId: body.account_id,
  }))
  await ctx.dispose()
}
