import { test, expect } from '@playwright/test'
import { login } from './helpers/auth'

test.describe('API Keys', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await page.goto('/keys')
  })

  test('create standard key shows generated key value', async ({ page }) => {
    // The keys page has a "Create Key" button (i18n: keys.createKey = "Create Key")
    await page.getByRole('button', { name: 'Create Key' }).click()

    // CreateKeyModal opens — form has Label htmlFor="key-name" with text "Name"
    const nameInput = page.getByLabel('Name')
    await nameInput.fill(`e2e-test-key-${Date.now()}`)

    // Submit via the dialog's Create Key button (the last "Create Key" button in DOM)
    await page.getByRole('button', { name: 'Create Key' }).last().click()

    // KeyCreatedModal appears with the raw key (starts with sk-)
    // and the warning "Save this key now"
    await expect(
      page.getByText(/save this key now/i)
    ).toBeVisible({ timeout: 10_000 })

    // The key itself is rendered in a <code> element
    await expect(
      page.locator('code').filter({ hasText: /sk-/ })
        .or(page.getByText(/sk-/))
    ).toBeVisible({ timeout: 10_000 })
  })
})
