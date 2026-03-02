# Web — Accounts Page (/accounts)

> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to CreateAccountModal | `web/app/accounts/page.tsx` form + `web/lib/api.ts` `createAccount()` + backend `account_handlers.rs` `CreateAccountRequest` | Form field → API body → Rust struct → DB migration if new column |
| Add column to accounts table | `web/app/accounts/page.tsx` table + `web/lib/types.ts` `Account` | Add `TableHead` + `TableCell` + extend type |
| Change session stale time | `web/lib/queries/accounts.ts` `accountSessionsQuery` `staleTime` | Adjust milliseconds (default: 30 000) |
| Add role option | `web/app/accounts/page.tsx` `CreateAccountModal` role `<select>` | Add `<option>` value; align with backend role enum |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/accounts/page.tsx` | Accounts management page (table + modals) |
| `web/lib/api.ts` | `api.accounts()`, `api.createAccount()`, `api.deleteAccount()`, `api.setAccountActive()`, `api.createResetLink()`, `api.accountSessions()`, `api.revokeSession()`, `api.revokeAllSessions()` |
| `web/lib/queries/accounts.ts` | `accountsQuery`, `accountSessionsQuery` TanStack Query definitions |
| `web/lib/types.ts` | `Account`, `CreateAccountRequest`, `CreateAccountResponse`, `SessionRecord` |
| `web/messages/en.json` | i18n keys under `accounts.*` |

---

## Page Layout

```
Title: "Accounts"  Subtitle: description                    [+ Create Account]

[Reset token banner — conditional, shown after createResetLink succeeds]

┌──────────────────────────────────────────────────────────────────────────────┐
│ Username   Name    Role    Department  Status   Last Login   Actions          │
│ alice      Alice   super   Eng         ◉ on     Mar 1 10:00  🛡 🔗 🗑         │
│ bob        Bob     admin   Ops         ◉ on     Mar 2 08:30  🛡 🔗 🗑         │
└──────────────────────────────────────────────────────────────────────────────┘
```

- `DataTable minWidth="700px"` — SSOT wrapper; never raw `<Table>`.
- **Role badge**: `super` → `variant="default"` (filled); `admin` → `variant="secondary"` (muted).
- **Status**: `Switch` — toggling calls `PATCH /v1/accounts/{id}/active`.
- **Actions per row** (three icon buttons):
  - `Shield` → opens `AccountSessionsModal` for that account.
  - `Link` → calls `POST /v1/accounts/{id}/reset-link`; shows reset token banner inline on the page.
  - `Trash2` → calls `DELETE /v1/accounts/{id}` (soft-delete assumed).

---

## Access Control

Requires JWT Bearer. No explicit role guard in the frontend component — access is enforced by the backend routes. The page is only reachable through the authenticated nav (super/admin role required).

---

## Component Structure

### `AccountsPage` (default export)

State:

```ts
const [showCreate, setShowCreate]             = useState(false)       // CreateAccountModal open
const [resetToken, setResetToken]             = useState<string|null>(null) // inline reset token banner
const [sessionsAccountId, setSessionsAccountId] = useState<string|null>(null) // SessionsModal target
```

Queries / mutations:

| Hook | Query key | Endpoint |
|------|-----------|----------|
| `useQuery(accountsQuery)` | `['accounts']` | `GET /v1/accounts` |
| `deleteMutation` | invalidates `['accounts']` | `DELETE /v1/accounts/{id}` |
| `activeMutation` | invalidates `['accounts']` | `PATCH /v1/accounts/{id}/active` |
| `resetMutation` | sets `resetToken` state | `POST /v1/accounts/{id}/reset-link` |

### `CreateAccountModal`

Two-phase dialog:

1. **Form phase** — fields: Username (required), Full name (required), Password (required), Email (optional), Role (`admin` default | `super`), Department (optional), Position (optional).
2. **Success phase** — shows `CreateAccountResponse.test_api_key` with `CopyButton` and warning "Save the test API key — it will never be shown again."

Required fields for submit: `username`, `password`, `name`. Submit disabled while `mutation.isPending`.

Closing resets all form state to defaults. Query invalidation: `['accounts']`.

### `AccountSessionsModal`

Opened per-account via the `Shield` icon button.

- Fetches `GET /v1/accounts/{id}/sessions` (enabled only when modal is open).
- Each session row shows: `ip_address`, `last_used_at`, `created_at`; individual `Trash2` revoke button.
- Footer: **Revoke All** (`DELETE /v1/accounts/{accountId}/sessions`) shown only when sessions exist; **Close** button always shown.
- Query invalidation on revoke: `['sessions', accountId]`.

### `CopyButton`

Inline utility component. Copies text to clipboard; shows `Check` icon for 2 s, then reverts to `Copy`.

---

## API Endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| `GET` | `/v1/accounts` | JWT | List all accounts |
| `POST` | `/v1/accounts` | JWT | Create account → returns `CreateAccountResponse` (includes `test_api_key`) |
| `DELETE` | `/v1/accounts/{id}` | JWT | Delete account |
| `PATCH` | `/v1/accounts/{id}/active` | JWT | Toggle `is_active` |
| `POST` | `/v1/accounts/{id}/reset-link` | JWT | Generate password reset token |
| `GET` | `/v1/accounts/{id}/sessions` | JWT | List active sessions for an account |
| `DELETE` | `/v1/sessions/{sessionId}` | JWT | Revoke a single session |
| `DELETE` | `/v1/accounts/{accountId}/sessions` | JWT | Revoke all sessions for an account |

---

## Data Types

```ts
interface Account {
  id: string               // UUIDv7 — unique identifier
  username: string
  name: string
  email: string | null
  role: 'super' | 'admin'
  department: string | null
  position: string | null
  is_active: boolean
  last_login_at: string | null  // ISO 8601 UTC
  created_at: string            // ISO 8601 UTC
}

interface CreateAccountRequest {
  username: string
  password: string
  name: string
  email?: string
  role?: string             // default 'admin'
  department?: string
  position?: string
}

interface CreateAccountResponse {
  id: string
  username: string
  role: string
  test_api_key: string      // shown once — never retrievable again
  created_at: string
}

interface SessionRecord {
  id: string
  ip_address: string | null
  created_at: string
  last_used_at: string | null
  expires_at: string
}
```

---

## TanStack Query Configuration

```ts
// accounts list — staleTime: Infinity (invalidated explicitly on mutations)
accountsQuery = queryOptions({ queryKey: ['accounts'], queryFn: () => api.accounts(), staleTime: Infinity })

// sessions per account — enabled only while modal is open
accountSessionsQuery(accountId, open) = queryOptions({
  queryKey: ['sessions', accountId],
  queryFn: () => api.accountSessions(accountId!),
  enabled: open && !!accountId,
  staleTime: 30_000,
  retry: false,
})
```

---

## Date Formatting

All timestamps use `fmtDatetime(value, tz)` from `web/lib/date.ts` with `useTimezone()`. Never call `.toLocaleString()` directly.

---

## i18n Keys (`accounts.*`)

```
accounts.title
accounts.description
accounts.createAccount
accounts.username
accounts.name
accounts.role
accounts.department
accounts.status
accounts.lastLogin
accounts.actions
accounts.sessions
accounts.noSessions
accounts.lastUsed
accounts.revokeSession
accounts.revokeAll
accounts.resetLink
accounts.noAccounts
```

Shared keys also used: `common.loading`, `common.error`, `common.delete`, `common.close`, `common.never`, `common.created`.

---

## Related Docs

- Backend auth + session management: `../backend/auth.md`
- JWT revocation (Valkey blocklist): see MEMORY — "JWT Sessions"
- Timezone formatting SSOT: `web/lib/date.ts`
- DataTable SSOT: `web/components/data-table.tsx`
