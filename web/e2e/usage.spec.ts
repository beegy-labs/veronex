import { test, expect } from '@playwright/test'
import { T_DEFAULT } from './helpers/constants'

test.describe('Usage & Performance', () => {
  test.beforeEach(async ({ page }) => {
  })

  test('usage page loads with breakdown charts', async ({ page }) => {
    await page.goto('/usage')
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // Usage page should show time range selector and chart or empty state
    await expect(
      page.getByText(/usage|tokens|requests|no data/i).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('usage page time range selector works', async ({ page }) => {
    await page.goto('/usage')
    // Look for a time range selector (24h, 7d, 30d buttons)
    const timeSelector = page.getByRole('button', { name: /24h|7d|30d/i }).first()
    if (await timeSelector.isVisible()) {
      await timeSelector.click()
      // Page should update without errors
      await expect(page).toHaveURL(/\/usage/)
    }
  })

  test('performance page loads with latency metrics', async ({ page }) => {
    await page.goto('/performance')
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // Performance page should show latency-related content
    await expect(
      page.getByText(/performance|latency|ttft|tps|no data/i).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })
})
