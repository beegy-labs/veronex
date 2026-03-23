import { test, expect } from '@playwright/test'
import { login } from './helpers/auth'
import { T_DEFAULT } from './helpers/constants'

test.describe('Navigation @smoke', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('sidebar navigation renders all main links', async ({ page }) => {
    // Verify all main nav items are present in the sidebar
    const navLinks = [
      /dashboard/i,
      /usage/i,
      /performance/i,
      /jobs/i,
      /keys/i,
      /servers/i,
      /mcp/i,
    ]
    for (const name of navLinks) {
      await expect(
        page.getByRole('link', { name }).or(page.getByText(name).first())
      ).toBeVisible({ timeout: T_DEFAULT })
    }
  })

  test('navigating between pages updates URL correctly', async ({ page }) => {
    // Navigate to jobs
    await page.getByRole('link', { name: /jobs/i }).first().click()
    await expect(page).toHaveURL(/\/jobs/)

    // Navigate to keys
    await page.getByRole('link', { name: /keys/i }).first().click()
    await expect(page).toHaveURL(/\/keys/)

    // Navigate back to overview
    await page.getByRole('link', { name: /dashboard/i }).first().click()
    await expect(page).toHaveURL(/\/overview/)
  })

  test('api-test route redirects to jobs', async ({ page }) => {
    await page.goto('/api-test')
    await expect(page).toHaveURL(/\/jobs/)
  })

  test('theme toggle switches between light and dark', async ({ page }) => {
    await page.goto('/overview')
    // Find the theme toggle button
    const themeToggle = page.getByRole('button', { name: /theme|dark|light/i }).first()
    if (await themeToggle.isVisible()) {
      const htmlBefore = await page.locator('html').getAttribute('data-theme')
      await themeToggle.click()
      // data-theme should change
      const htmlAfter = await page.locator('html').getAttribute('data-theme')
      expect(htmlAfter).not.toEqual(htmlBefore)
    }
  })
})
