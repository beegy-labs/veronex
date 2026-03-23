import { test, expect } from '@playwright/test'
import { T_DEFAULT, T_SHORT } from './helpers/constants'

test.describe('GPU Servers', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/servers')
  })

  test('servers page loads with table or empty state', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    await expect(
      page.locator('table')
        .or(page.getByText(/no servers|register/i))
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('can open register server dialog', async ({ page }) => {
    const addButton = page.getByRole('button', { name: /add|register|create/i }).first()
    if (await addButton.isVisible()) {
      await addButton.click()
      await expect(
        page.getByRole('dialog')
          .or(page.getByLabel(/name|hostname/i))
      ).toBeVisible({ timeout: T_SHORT })
    }
  })
})
