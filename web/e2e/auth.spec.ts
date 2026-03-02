import { test, expect } from '@playwright/test'
import { login, TEST_CREDENTIALS } from './helpers/auth'

test.describe('Authentication', () => {
  test('login with valid credentials redirects to overview', async ({ page }) => {
    await login(page)
    await expect(page).toHaveURL(/\/overview/)
  })

  test('login with invalid credentials shows error', async ({ page }) => {
    await page.goto('/login')
    // Login page uses explicit Label elements with htmlFor="username" / htmlFor="password"
    await page.getByLabel('Username').fill('invalid-user')
    await page.getByLabel('Password').fill('wrong-password')
    await page.getByRole('button', { name: /sign in/i }).click()
    // Should stay on login page and show "Invalid username or password" error text
    await expect(page).toHaveURL(/\/login/)
    await expect(
      page.locator('text=Invalid username or password')
    ).toBeVisible({ timeout: 5_000 })
  })

  test('unauthenticated access to overview redirects to login', async ({ page }) => {
    await page.goto('/overview')
    await expect(page).toHaveURL(/\/login/)
  })
})
