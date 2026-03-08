import { test, expect } from '@playwright/test'
import { login } from './helpers/auth'
import { T_DEFAULT, T_LONG } from './helpers/constants'

test.describe('Audit Trail', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await page.goto('/audit')
  })

  test('audit page loads and shows event table or empty state', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // Audit page should show a table of events or an empty state
    await expect(
      page.locator('table')
        .or(page.getByText(/no audit|no events|loading/i))
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('audit events include login action from test setup', async ({ page }) => {
    // After login, there should be at least one "login" audit event
    // (the login we just did in beforeEach)
    await expect(
      page.getByText(/login/i).first()
    ).toBeVisible({ timeout: T_LONG })
  })
})
