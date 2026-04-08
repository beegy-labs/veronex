import { Page } from '@playwright/test'
import { T_DEFAULT, TEST_USERNAME, TEST_PASSWORD } from './constants'

export const TEST_CREDENTIALS = {
  username: TEST_USERNAME,
  password: TEST_PASSWORD,
}

export async function login(page: Page) {
  await page.goto('/login')
  // Login page uses htmlFor="username" / htmlFor="password" Labels
  // and a "Sign in" submit button (login/page.tsx)
  await page.getByLabel('Username', { exact: true }).fill(TEST_CREDENTIALS.username)
  await page.getByLabel('Password', { exact: true }).fill(TEST_CREDENTIALS.password)
  await page.getByRole('button', { name: /sign in/i }).click()
  // After login the router pushes to '/' which redirects to /overview
  await page.waitForURL('**/overview', { timeout: T_DEFAULT })
}

export async function logout(page: Page) {
  // Clear auth state by navigating to login — tokens are stored in memory/cookies
  await page.goto('/login')
}
