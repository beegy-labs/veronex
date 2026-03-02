import { test, expect } from '@playwright/test'
import { login } from './helpers/auth'

test.describe('Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('overview page loads with KPI cards', async ({ page }) => {
    await page.goto('/overview')
    // Overview page renders an h1 with the dashboard nav key ("Dashboard")
    // and then DashboardTab with KPI cards for Waiting/Running/provider counts
    await expect(page.getByRole('heading', { level: 1, name: /dashboard/i })).toBeVisible({ timeout: 10_000 })
    // At least one of the known KPI labels from the overview i18n strings should appear
    await expect(
      page.getByText(/waiting|running|providers|active keys/i).first()
    ).toBeVisible({ timeout: 10_000 })
  })

  test('jobs page loads and shows table or empty state', async ({ page }) => {
    await page.goto('/jobs')
    await expect(page).toHaveURL(/\/jobs/)
    // Either a table with job rows or the "no jobs" empty state should render
    await expect(
      page.locator('table').or(page.getByText(/no jobs|loading/i))
    ).toBeVisible({ timeout: 10_000 })
  })

  test('keys page loads and shows table or empty state', async ({ page }) => {
    await page.goto('/keys')
    await expect(page).toHaveURL(/\/keys/)
    // Either the keys table or the "No API keys yet" empty state should render
    await expect(
      page.locator('table').or(page.getByText(/no api keys yet|loading keys/i))
    ).toBeVisible({ timeout: 10_000 })
  })
})
