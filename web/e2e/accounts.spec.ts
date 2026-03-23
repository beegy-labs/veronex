import { test, expect } from '@playwright/test'
import { T_DEFAULT, T_SHORT } from './helpers/constants'

test.describe('Accounts Management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/accounts')
  })

  test('accounts page loads and shows current user', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // Should show at least the logged-in admin account
    await expect(
      page.locator('table').or(page.getByText(/admin/i))
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('can open create account dialog', async ({ page }) => {
    const createButton = page.getByRole('button', { name: /create|add/i }).first()
    await expect(createButton).toBeVisible({ timeout: T_DEFAULT })
    await createButton.click()

    // Dialog should appear with username and password fields
    await expect(
      page.getByRole('dialog')
        .or(page.getByLabel(/username/i))
    ).toBeVisible({ timeout: T_SHORT })
  })

  test('account list shows role column', async ({ page }) => {
    // The accounts table should have a role column (super/admin)
    await expect(
      page.getByText(/super|admin/i).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })
})
