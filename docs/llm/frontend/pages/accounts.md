# Web -- Accounts Page (/accounts)

> CDD Layer 2 | **Last Updated**: 2026-03-04

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to CreateAccountModal | `web/app/accounts/page.tsx` form + `web/lib/api.ts` `createAccount()` + backend `account_handlers.rs` `CreateAccountRequest` | Form field -> API body -> Rust struct -> DB migration if new column |
| Add column to accounts table | `web/app/accounts/page.tsx` table + `web/lib/types.ts` `Account` | Add `TableHead` + `TableCell` + extend type |
| Change session stale time | `web/lib/queries/accounts.ts` `accountSessionsQuery` `staleTime` | Adjust ms (default: 30 000) |
| Add role option | `web/app/accounts/page.tsx` `CreateAccountModal` role `<select>` | Add `<option>` value; align with backend role enum |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/accounts/page.tsx` | Accounts management page (table + modals) |
| `web/lib/api.ts` | `api.accounts()`, `createAccount()`, `deleteAccount()`, `setAccountActive()`, `createResetLink()`, `accountSessions()`, `revokeSession()`, `revokeAllSessions()` |
| `web/lib/queries/accounts.ts` | `accountsQuery`, `accountSessionsQuery` TanStack Query defs |
| `web/lib/types.ts` | `Account`, `CreateAccountRequest`, `CreateAccountResponse`, `SessionRecord` |
| `web/messages/en.json` | i18n keys under `accounts.*` |

## Page Layout

| Element | Detail |
|---------|--------|
| Header | Title + description + `[+ Create Account]` button |
| Banner | Conditional reset-token banner (shown after `createResetLink` succeeds) |
| Table | Columns: Username, Name, Role, Department, Status, Last Login, Actions |
| Role badge | `super` -> `variant="default"`; `admin` -> `variant="secondary"` |
| Status | `Switch` toggle -> `PATCH /v1/accounts/{id}/active` |
| Actions | Shield (sessions modal), Link (reset-link), Trash2 (delete) |

Table wrapped in `DataTable minWidth="700px"`.

## Access Control

Requires JWT Bearer. No frontend role guard -- enforced by server routes. Page reachable only through authenticated nav (super/admin).

## Component Structure

### `AccountsPage` (default export)

| State | Type | Purpose |
|-------|------|---------|
| `showCreate` | `boolean` | CreateAccountModal open |
| `resetToken` | `string\|null` | Inline reset token banner |
| `sessionsAccountId` | `string\|null` | SessionsModal target |

| Hook | Query key | Endpoint |
|------|-----------|----------|
| `useQuery(accountsQuery)` | `['accounts']` | `GET /v1/accounts` |
| `deleteMutation` | invalidates `['accounts']` | `DELETE /v1/accounts/{id}` |
| `activeMutation` | invalidates `['accounts']` | `PATCH /v1/accounts/{id}/active` |
| `resetMutation` | sets `resetToken` state | `POST /v1/accounts/{id}/reset-link` |

### `CreateAccountModal`

Two-phase dialog:
1. **Form** -- Username (req), Full name (req), Password (req), Email (opt), Role (`admin` default / `super`), Department (opt), Position (opt). Submit disabled while `mutation.isPending`.
2. **Success** -- Shows `CreateAccountResponse.test_api_key` with `CopyButton` + warning "Save the test API key -- never shown again."

Closing resets form state. Invalidates `['accounts']`.

### `AccountSessionsModal`

Opened per-account via Shield icon. Fetches `GET /v1/accounts/{id}/sessions` (enabled only when modal open). Each row: `ip_address`, `last_used_at`, `created_at` + revoke button. Footer: **Revoke All** (shown when sessions exist) + **Close**. Invalidates `['sessions', accountId]`.

### `CopyButton`

Copies text to clipboard; shows Check icon for 2s, reverts to Copy.

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/v1/accounts` | List all accounts |
| `POST` | `/v1/accounts` | Create account (returns `test_api_key`) |
| `DELETE` | `/v1/accounts/{id}` | Delete account |
| `PATCH` | `/v1/accounts/{id}/active` | Toggle `is_active` |
| `POST` | `/v1/accounts/{id}/reset-link` | Generate password reset token |
| `GET` | `/v1/accounts/{id}/sessions` | List active sessions |
| `DELETE` | `/v1/sessions/{sessionId}` | Revoke single session |
| `DELETE` | `/v1/accounts/{accountId}/sessions` | Revoke all sessions |

All endpoints require JWT auth.

## Data Types

**Account**: `id` (UUIDv7), `username`, `name`, `email?`, `department?`, `position?`, `is_active` (bool), `last_login_at?` (ISO 8601), `created_at` (ISO 8601). Roles via `account_roles` N:N join table (`super`, `viewer`, or custom roles).

**CreateAccountRequest**: `username` (req), `password` (req), `name` (req), `email`, `role_ids` (UUID array, default: viewer role), `department`, `position`.

**CreateAccountResponse**: `id`, `username`, `role`, `test_api_key` (shown once, never retrievable), `created_at`.

**SessionRecord**: `id`, `ip_address?`, `created_at`, `last_used_at?`, `expires_at`.

## TanStack Query Config

| Query | Key | staleTime | Notes |
|-------|-----|-----------|-------|
| `accountsQuery` | `['accounts']` | `Infinity` | Invalidated on mutations |
| `accountSessionsQuery(id, open)` | `['sessions', id]` | `30_000` | `enabled: open && !!id`, `retry: false` |

## Date Formatting

All timestamps use `fmtDatetime(value, tz)` from `web/lib/date.ts` with `useTimezone()`. Never call `.toLocaleString()` directly.

## i18n Keys

Namespace: `accounts.*` -- `title`, `description`, `createAccount`, `username`, `name`, `role`, `department`, `status`, `lastLogin`, `actions`, `sessions`, `noSessions`, `lastUsed`, `revokeSession`, `revokeAll`, `resetLink`, `noAccounts`.

Shared: `common.loading`, `common.error`, `common.delete`, `common.close`, `common.never`, `common.created`.

## Related Docs

- Backend auth + sessions: `../../auth/jwt-sessions.md`
- Timezone formatting SSOT: `web/lib/date.ts`
- DataTable SSOT: `web/components/data-table.tsx`
