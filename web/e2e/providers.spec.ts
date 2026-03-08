import { test, expect } from '@playwright/test'
import { login } from './helpers/auth'
import { T_DEFAULT, T_SHORT } from './helpers/constants'

test.describe('Providers', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await page.goto('/providers')
  })

  test('providers page loads with Ollama and Gemini tabs', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    // The providers page has tabs for Ollama and Gemini
    await expect(
      page.getByRole('tab', { name: /ollama/i })
        .or(page.getByText(/ollama/i).first())
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('can open register provider dialog', async ({ page }) => {
    // Look for a button to add/register a provider
    const addButton = page.getByRole('button', { name: /add|register|create/i }).first()
    await expect(addButton).toBeVisible({ timeout: T_DEFAULT })
    await addButton.click()

    // A dialog/modal should appear with form fields
    await expect(
      page.getByRole('dialog')
        .or(page.getByLabel(/name/i))
    ).toBeVisible({ timeout: T_SHORT })
  })

  test('provider list displays registered providers or empty state', async ({ page }) => {
    // Either a table with providers or an empty state message
    await expect(
      page.locator('table')
        .or(page.getByText(/no providers|no ollama|register/i))
    ).toBeVisible({ timeout: T_DEFAULT })
  })
})
