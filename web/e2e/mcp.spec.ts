import { test, expect } from '@playwright/test'
import { T_DEFAULT, T_SHORT, testId } from './helpers/constants'

test.describe('MCP Servers', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/mcp')
  })

  test('MCP page loads with heading and register button', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: T_DEFAULT })
    const addButton = page.getByRole('button', { name: /add|register|connect/i }).first()
    await expect(addButton).toBeVisible({ timeout: T_DEFAULT })
  })

  test('server list displays table or empty state', async ({ page }) => {
    // Either the servers table or the empty-state dashed card should be visible
    await expect(
      page.locator('table').or(page.locator('.border-dashed'))
    ).toBeVisible({ timeout: T_DEFAULT })
  })

  test('can open register MCP server dialog', async ({ page }) => {
    const addButton = page.getByRole('button', { name: /add|register|connect/i }).first()
    await addButton.click()

    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: T_SHORT })
    await expect(dialog.getByLabel(/name/i)).toBeVisible({ timeout: T_SHORT })
    await expect(dialog.getByLabel(/url/i)).toBeVisible({ timeout: T_SHORT })
  })

  test('register dialog closes on cancel', async ({ page }) => {
    const addButton = page.getByRole('button', { name: /add|register|connect/i }).first()
    await addButton.click()

    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: T_SHORT })

    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).not.toBeVisible({ timeout: T_SHORT })
  })

  test('register and delete MCP server', async ({ page }) => {
    const name = `test-mcp-${testId()}`
    const url = 'http://localhost:3100'

    // Open register dialog
    const addButton = page.getByRole('button', { name: /add|register|connect/i }).first()
    await addButton.click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: T_SHORT })

    // Fill form
    await dialog.getByLabel(/name/i).fill(name)
    await dialog.getByLabel(/url/i).fill(url)

    // Submit
    await dialog.getByRole('button', { name: /add|register|save|connect/i }).last().click()
    await expect(dialog).not.toBeVisible({ timeout: T_DEFAULT })

    // Verify server appears in table
    await expect(page.getByText(name)).toBeVisible({ timeout: T_DEFAULT })

    // Delete the server — accept the native confirm() dialog
    page.once('dialog', (dialog) => dialog.accept())
    const row = page.locator('tr', { hasText: name })
    await row.getByRole('button', { name: /delete/i }).click()

    await expect(page.getByText(name)).not.toBeVisible({ timeout: T_DEFAULT })
  })

  test('enable/disable toggle works', async ({ page }) => {
    // Only run if there are servers in the table
    const rows = page.locator('tbody tr')
    const count = await rows.count()
    if (count === 0) {
      test.skip()
      return
    }

    const toggle = rows.first().getByRole('switch')
    if (!(await toggle.isVisible({ timeout: T_SHORT }).catch(() => false))) return

    const wasChecked = await toggle.isChecked()
    await toggle.click()
    // Toggle state should change
    await expect(toggle).toHaveAttribute(
      'aria-checked',
      wasChecked ? 'false' : 'true',
      { timeout: T_DEFAULT }
    )
  })
})
