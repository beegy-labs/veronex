import { test, expect } from '@playwright/test'
import { T_DEFAULT } from './helpers/constants'

test.describe('Flow', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/flow')
  })

  test('flow page loads with heading', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
  })

  test('flow page renders visualization or empty state', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    await expect(
      page.locator('svg').or(page.getByText(/no data|no active|waiting/i)).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })
})
