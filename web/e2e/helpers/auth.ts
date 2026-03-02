import { Page } from '@playwright/test'

export const TEST_CREDENTIALS = {
  username: process.env.E2E_USERNAME ?? 'admin',
  password: process.env.E2E_PASSWORD ?? 'changeme',
}

export async function login(page: Page) {
  await page.goto('/login')
  // Login page uses htmlFor="username" / htmlFor="password" Labels
  // and a "Sign in" submit button (login/page.tsx)
  await page.getByLabel('Username').fill(TEST_CREDENTIALS.username)
  await page.getByLabel('Password').fill(TEST_CREDENTIALS.password)
  await page.getByRole('button', { name: /sign in/i }).click()
  // After login the router pushes to '/' which redirects to /overview
  await page.waitForURL('**/overview', { timeout: 10_000 })
}

export async function logout(page: Page) {
  // Clear auth state by navigating to login — tokens are stored in memory/cookies
  await page.goto('/login')
}
