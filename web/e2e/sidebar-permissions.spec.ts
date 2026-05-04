import { test, expect, type APIRequestContext, type Page } from '@playwright/test'
import { API_BASE_URL, T_DEFAULT, testId } from './helpers/constants'
import { apiLogin } from './helpers/api'

// Cover PR #76 sidebar coherence + URL-direct guard.
//
// What this asserts:
//   1. A non-super role with only `dashboard_view` cannot see `/mcp` in the
//      sidebar AND typing `/mcp` directly is bounced back to `/overview`.
//   2. Distinct permission combinations show distinct sidebar items
//      (sidebar reflects the SSOT, not a hard-coded list).
//   3. Every visible sidebar link routes to a non-403 page (no zombie pages).

interface DelegatedAccount {
  username: string
  password: string
  roleId: string
  accountId: string
}

async function adminToken(request: APIRequestContext): Promise<string> {
  const tokens = await apiLogin(request)
  return tokens.accessToken
}

async function createDelegatedAccount(
  request: APIRequestContext,
  permissions: string[],
): Promise<DelegatedAccount> {
  const adminTk = await adminToken(request)
  const auth = { Authorization: `Bearer ${adminTk}` }
  const suffix = testId()
  const roleName = `e2e-sidebar-${suffix}`
  const username = `e2esb${suffix}`
  const password = 'TestPass123!'

  const roleRes = await request.post(`${API_BASE_URL}/v1/roles`, {
    headers: auth,
    data: { name: roleName, permissions },
  })
  if (!roleRes.ok()) throw new Error(`Role create failed: ${roleRes.status()}`)
  const role = await roleRes.json()

  const acctRes = await request.post(`${API_BASE_URL}/v1/accounts`, {
    headers: auth,
    data: { username, password, name: 'Sidebar', role_ids: [role.id] },
  })
  if (!acctRes.ok()) throw new Error(`Account create failed: ${acctRes.status()}`)
  const acct = await acctRes.json()

  return { username, password, roleId: role.id, accountId: acct.id }
}

async function deleteDelegated(
  request: APIRequestContext,
  d: DelegatedAccount,
): Promise<void> {
  const adminTk = await adminToken(request)
  const auth = { Authorization: `Bearer ${adminTk}` }
  await request.delete(`${API_BASE_URL}/v1/accounts/${d.accountId}`, { headers: auth })
  await request.delete(`${API_BASE_URL}/v1/roles/${d.roleId}`, { headers: auth })
}

async function loginAs(page: Page, username: string, password: string) {
  await page.goto('/login')
  await page.getByLabel('Username', { exact: true }).fill(username)
  await page.getByLabel('Password', { exact: true }).fill(password)
  await page.getByRole('button', { name: /sign in/i }).click()
  await page.waitForURL('**/overview', { timeout: T_DEFAULT })
}

test.describe('Sidebar permission coherence (PR #76)', () => {
  // Reset to a fresh storage state — global setup logs in as super.
  test.use({ storageState: { cookies: [], origins: [] } })

  test('viewer-only role: /mcp hidden in sidebar AND direct URL bounces to /overview', async ({
    page,
    request,
  }) => {
    const acct = await createDelegatedAccount(request, ['dashboard_view'])
    try {
      await loginAs(page, acct.username, acct.password)

      // Sidebar: /mcp link must not be present
      const mcpLink = page.getByRole('link', { name: /mcp/i })
      await expect(mcpLink).toHaveCount(0)

      // Direct URL: /mcp must redirect to /overview (or land on a non-403 page).
      // The page guard reads ROUTE_PERMISSION and pushes back to /overview.
      await page.goto('/mcp')
      await page.waitForURL('**/overview', { timeout: T_DEFAULT })
      await expect(page).toHaveURL(/\/overview$/)
    } finally {
      await deleteDelegated(request, acct)
    }
  })

  test('mcp_manage-only role: /mcp visible AND clickable, /accounts hidden', async ({
    page,
    request,
  }) => {
    const acct = await createDelegatedAccount(request, [
      'dashboard_view',
      'mcp_manage',
    ])
    try {
      await loginAs(page, acct.username, acct.password)

      await expect(
        page.getByRole('link', { name: /mcp/i }).first(),
      ).toBeVisible({ timeout: T_DEFAULT })
      await expect(page.getByRole('link', { name: /accounts/i })).toHaveCount(0)

      await page.getByRole('link', { name: /mcp/i }).first().click()
      await expect(page).toHaveURL(/\/mcp/)
      // Must not be a 403 page — header should render
      await expect(page.locator('text=/forbidden|403/i')).toHaveCount(0)
    } finally {
      await deleteDelegated(request, acct)
    }
  })

  test('every sidebar link routes to a non-403 page (no zombie items)', async ({
    page,
    request,
  }) => {
    // Multi-permission account exercises the most surface
    const acct = await createDelegatedAccount(request, [
      'dashboard_view',
      'api_test',
      'provider_manage',
      'key_manage',
      'mcp_manage',
      'account_manage',
      'audit_view',
    ])
    try {
      await loginAs(page, acct.username, acct.password)

      // Collect all sidebar links visible right now.
      const links = await page
        .locator('nav a[href^="/"]')
        .evaluateAll((nodes) =>
          nodes
            .map((n) => (n as HTMLAnchorElement).getAttribute('href'))
            .filter(
              (h): h is string =>
                !!h && h.startsWith('/') && !h.startsWith('/login') && h !== '/',
            ),
        )

      expect(links.length).toBeGreaterThan(0)

      const seen = new Set<string>()
      for (const href of links) {
        if (seen.has(href)) continue
        seen.add(href)
        await page.goto(href)
        // Either landed on the same href, or got redirected — both are
        // acceptable, what matters is we never see a 403 surface.
        await expect(page.locator('text=/forbidden|403/i')).toHaveCount(0, {
          timeout: T_DEFAULT,
        })
      }
    } finally {
      await deleteDelegated(request, acct)
    }
  })
})
