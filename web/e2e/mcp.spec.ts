import { test, expect } from '@playwright/test'
import { apiLogin, authedRequest } from './helpers/api'
import { T_DEFAULT, T_SHORT, T_LONG, testId } from './helpers/constants'

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
      page.locator('table').first().or(page.locator('.border-dashed'))
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

  test('register and delete MCP server', async ({ page, request }) => {
    const name = `test-mcp-${testId()}`
    const url = 'http://localhost:3100'
    const tokens = await apiLogin(request)
    const api = authedRequest(request, tokens.accessToken)

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

    try {
      // Delete the server via ConfirmDialog
      const row = page.locator('tr', { hasText: name })
      await row.getByRole('button', { name: /delete/i }).click()
      const confirmDialog = page.getByRole('dialog')
      await expect(confirmDialog).toBeVisible({ timeout: T_SHORT })
      await confirmDialog.getByRole('button', { name: /delete|confirm/i }).last().click()
      await expect(confirmDialog).not.toBeVisible({ timeout: T_DEFAULT })

      await expect(page.locator('tr', { hasText: name })).not.toBeVisible({ timeout: T_DEFAULT })
    } finally {
      // Always cleanup via API in case UI deletion fails
      const listRes = await api.get('/v1/mcp/servers')
      const servers: Array<{ id: string; name: string }> = await listRes.json()
      const created = servers.find((s) => s.name === name)
      if (created) await api.delete(`/v1/mcp/servers/${created.id}`)
    }
  })

  test('registered server shows status and tool count in table', async ({ page, request }) => {
    const name = `e2e-mcp-ui-${testId()}`
    const tokens = await apiLogin(request)
    const api = authedRequest(request, tokens.accessToken)

    // Register via UI
    const addButton = page.getByRole('button', { name: /register/i }).first()
    await addButton.click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: T_SHORT })

    await dialog.locator('#mcp-name').fill(name)
    await dialog.locator('#mcp-url').fill('http://localhost:3100')
    await dialog.getByRole('button', { name: /register/i }).last().click()
    await expect(dialog).not.toBeVisible({ timeout: T_DEFAULT })

    try {
      // Row appears in table
      const row = page.locator('tr', { hasText: name })
      await expect(row).toBeVisible({ timeout: T_DEFAULT })

      // Status cell shows "Online" or "Offline"
      await expect(row.getByText(/online|offline/i)).toBeVisible({ timeout: T_DEFAULT })

      // Tool count cell shows "<N> tools"
      await expect(row.getByText(/\d+\s+tools/i)).toBeVisible({ timeout: T_DEFAULT })

      // If server comes online within T_LONG, verify tool count > 0
      const statusCell = row.getByText(/online/i)
      const isOnline = await statusCell.isVisible({ timeout: T_LONG }).catch(() => false)
      if (isOnline) {
        // tool_count should be > 0 for a live weather-mcp server
        const toolText = await row.getByText(/\d+\s+tools/i).textContent()
        const toolCount = parseInt(toolText ?? '0')
        expect(toolCount).toBeGreaterThan(0)
      }
    } finally {
      // Always cleanup via API
      const listRes = await api.get('/v1/mcp/servers')
      const servers: Array<{ id: string; name: string }> = await listRes.json()
      const created = servers.find((s) => s.name === name)
      if (created) await api.delete(`/v1/mcp/servers/${created.id}`)
    }
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
      { timeout: T_LONG }
    )
  })
})
