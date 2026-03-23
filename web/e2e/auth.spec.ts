import { test, expect } from '@playwright/test'
import { login, TEST_CREDENTIALS } from './helpers/auth'
import { T_SHORT } from './helpers/constants'

// Auth tests require a clean (unauthenticated) browser state
test.use({ storageState: { cookies: [], origins: [] } })

test.describe('Authentication @smoke', () => {
  test('login with valid credentials redirects to overview', async ({ page }) => {
    await login(page)
    await expect(page).toHaveURL(/\/overview/)
  })

  test('login with invalid credentials shows error', async ({ page }) => {
    await page.goto('/login')
    await page.getByLabel('Username').fill('invalid-user')
    await page.getByLabel('Password').fill('wrong-password')
    await page.getByRole('button', { name: /sign in/i }).click()
    await expect(page).toHaveURL(/\/login/)
    await expect(
      page.getByText(/invalid username or password/i)
    ).toBeVisible({ timeout: T_SHORT })
  })

  test('unauthenticated access to overview redirects to login', async ({ page }) => {
    await page.goto('/overview')
    await expect(page).toHaveURL(/\/login/)
  })
})
