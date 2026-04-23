# Frontend Patterns — E2E Test Patterns

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## E2E Test Patterns

### Test Constants

All E2E test constants live in `web/e2e/helpers/constants.ts`:

| Export | Purpose |
|--------|---------|
| `API_BASE_URL` | Backend URL (env: `PLAYWRIGHT_API_URL`, default: `localhost:3001`) |
| `TEST_USERNAME` | Login username (env: `E2E_USERNAME`, default: `admin`) |
| `TEST_PASSWORD` | Login password (env: `E2E_PASSWORD`, default: `changeme`) |
| `testId()` | Generate unique 8-char suffix for test resources |

### Resource Cleanup

All tests that create or modify resources MUST use `try/finally` to guarantee cleanup even on assertion failure:

```typescript
// UI create + API fallback cleanup (preferred for UI tests)
test('create and delete server', async ({ page, request }) => {
  const name = `e2e-${testId()}`
  const tokens = await apiLogin(request)
  const api = authedRequest(request, tokens.accessToken)

  // Create via UI
  await page.getByRole('button', { name: /register/i }).click()
  await page.getByLabel(/name/i).fill(name)
  await page.getByRole('button', { name: /register/i }).last().click()

  try {
    // Verify + interact
    await expect(page.getByText(name)).toBeVisible({ timeout: T_DEFAULT })
    // ... more assertions ...
  } finally {
    // Always cleanup via API regardless of UI assertion outcome
    const list = await api.get('/v1/resource').then(r => r.json())
    const created = list.find((s: { name: string }) => s.name === name)
    if (created) await api.delete(`/v1/resource/${created.id}`)
  }
})

// Pure API test (simpler pattern)
let createdId: string | undefined
try {
  const res = await api.post('/v1/keys', { name: `e2e-${testId()}` })
  createdId = (await res.json()).id
  // ... assertions ...
} finally {
  if (createdId) await api.delete(`/v1/keys/${createdId}`)
}
```

### API Auth Helper Pattern

Tests that need direct API calls use `apiLogin` + `authedRequest` from `web/e2e/helpers/api.ts`:

```typescript
import { apiLogin, authedRequest } from './helpers/api'

test('...', async ({ page, request }) => {
  const tokens = await apiLogin(request)      // POST /v1/auth/login
  const api = authedRequest(request, tokens.accessToken)  // adds Authorization header

  const res = await api.post('/v1/mcp/servers', { data: { name, url } })
  // use api.get / api.post / api.patch / api.delete
})
```

Fixture: always destructure `{ page, request }` — `request` is the Playwright `APIRequestContext`.

### Dialog Interaction (ConfirmDialog)

NEVER use `page.once('dialog', ...)` — this intercepts native browser dialogs, not app-rendered React modals:

```typescript
// CORRECT — query the React-rendered ConfirmDialog
const confirmDialog = page.getByRole('dialog')
await expect(confirmDialog).toBeVisible({ timeout: T_SHORT })
await confirmDialog.getByRole('button', { name: /delete|confirm/i }).last().click()
await expect(confirmDialog).not.toBeVisible({ timeout: T_DEFAULT })

// WRONG — dead code for React-managed modals
page.once('dialog', (dialog) => dialog.accept()) // ✗ only fires for window.confirm()
```

---

