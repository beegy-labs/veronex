import { test, expect } from '@playwright/test'
import { testId, T_DEFAULT } from './helpers/constants'

test.describe('API Keys', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/keys')
  })

  test('create standard key shows generated key value', async ({ page }) => {
    // The keys page has a "Create Key" button (i18n: keys.createKey = "Create Key")
    await page.getByRole('button', { name: 'Create Key' }).click()

    // CreateKeyModal opens — form has Label htmlFor="key-name" with text "Name"
    const nameInput = page.getByLabel('Name')
    await nameInput.fill(`e2e-test-key-${testId()}`)

    // Submit via the dialog's Create Key button (the last "Create Key" button in DOM)
    await page.getByRole('button', { name: 'Create Key' }).last().click()

    // KeyCreatedModal appears with the raw key (starts with sk-)
    // and the warning "Save this key now"
    await expect(
      page.getByText(/save this key now/i)
    ).toBeVisible({ timeout: T_DEFAULT })

    // The key itself is rendered in a <code> element (prefix: vnx_)
    await expect(
      page.locator('code').filter({ hasText: /vnx_/ })
        .or(page.getByText(/vnx_/))
        .first()
    ).toBeVisible({ timeout: T_DEFAULT })
  })
})
