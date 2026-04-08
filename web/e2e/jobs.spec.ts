import { test, expect } from '@playwright/test'
import { T_DEFAULT, T_SHORT } from './helpers/constants'

test.describe('Jobs', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/jobs')
  })

  test('jobs page loads with heading and table or empty state', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    await expect(
      page.locator('table').or(page.getByText(/no jobs|no data/i)).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('jobs page has filter controls', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // Filter/search controls should be present
    await expect(
      page.getByRole('combobox').or(page.getByRole('button', { name: /filter|status/i })).first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('clicking a job row opens detail modal or panel', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    const rows = page.locator('tbody tr')
    const count = await rows.count()
    if (count === 0) return // no jobs to click

    await rows.first().click()
    await expect(
      page.getByRole('dialog').or(page.locator('[data-testid="job-detail"]')).first()
    ).toBeVisible({ timeout: T_SHORT })
  })
})
