# Web — Setup / Bootstrap Page (/setup)

> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add optional field to setup form | `web/app/setup/page.tsx` form + `web/lib/api.ts` `setup()` + backend `setup_handlers.rs` | Add `<Input>` + include in `api.setup()` body |
| Change post-setup redirect target | `web/app/setup/page.tsx` `router.push('/')` | Replace `'/'` with desired path |
| Change password minimum length | `web/app/setup/page.tsx` client validation + `<Input minLength>` | Align with backend constraint |
| Handle "already set up" differently | `web/app/setup/page.tsx` `catch` block, `409` branch | Redirect to `/login` or show custom UI instead of inline error |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/setup/page.tsx` | Bootstrap wizard page |
| `web/lib/api.ts` | `api.setupStatus()`, `api.setup()` — both use `fetchPublic` (no auth header) |
| `web/lib/auth.ts` | `setTokens(resp)` — persists JWT access + refresh tokens after setup |
| `web/lib/types.ts` | `LoginResponse` — shape returned by `POST /v1/setup` |

---

## Page Purpose

One-time bootstrap wizard shown on first run, before any admin account exists. Creates the initial `super` role account and immediately logs in by storing the returned JWT tokens.

The page is **publicly accessible** (no auth required). It uses `fetchPublic` (no `Authorization` header) for both status check and submission.

---

## State Machine

```
              ┌─────────────────────────────────────────────────────────┐
              │                    SetupPage                            │
              │                                                         │
  [mount] ──→ │  idle                                                   │
              │   ↓ user fills form + clicks "Create account"           │
              │  loading  ←─────────────────────────────────────────────┤
              │   │                                                      │
              │   ├─ success ──→ setTokens(resp) ──→ router.push('/')   │
              │   │                                                      │
              │   └─ error ────→ show inline error message              │
              │       ├─ 409: "Setup already completed. Please sign in."│
              │       └─ other: "Setup failed. Please try again."       │
              └─────────────────────────────────────────────────────────┘
```

State variables:

```ts
const [username, setUsername] = useState('')
const [password, setPassword] = useState('')
const [confirm,  setConfirm]  = useState('')
const [error,    setError]    = useState<string | null>(null)
const [loading,  setLoading]  = useState(false)
```

No multi-step wizard — a single form with all required fields.

---

## Bootstrap Flow

1. User navigates to `/setup` (typically redirected from `layout.tsx` after `GET /v1/setup/status` returns `{ needs_setup: true }`).
2. User enters **Username** and **Password** (min 8 characters) + **Confirm Password**.
3. Client validates:
   - `password.length < 8` → inline error.
   - `password !== confirm` → inline error.
4. On valid submit: `POST /v1/setup` with `{ username, password }`.
5. On success: `setTokens(resp)` stores access + refresh tokens → `router.push('/')`.
6. On `409` response: setup was already completed; user must sign in at `/login`.

The created account has `role = 'super'`.

---

## API Endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| `GET` | `/v1/setup/status` | None (public) | Check whether first-run setup is needed |
| `POST` | `/v1/setup` | None (public) | Create super admin account + return session tokens |

### `GET /v1/setup/status`

```ts
// Response
{ needs_setup: boolean }
```

Used by `layout.tsx` (or middleware) to redirect unauthenticated users to `/setup` when `needs_setup: true`.

### `POST /v1/setup`

```ts
// Request body
{ username: string; password: string }

// Response — LoginResponse
{
  access_token:  string
  token_type:    string    // "Bearer"
  account_id:    string
  username:      string
  role:          string    // "super"
  refresh_token: string
}
```

Returns `409 Conflict` if setup has already been completed.

---

## Post-Submit Behavior

On success the page calls `setTokens(resp)` (from `web/lib/auth.ts`) which persists both `access_token` and `refresh_token` (e.g., in `localStorage` or cookies per `auth.ts` implementation). The router then navigates to `/` (the dashboard root).

The page itself does **not** poll `GET /v1/setup/status` — that check is the responsibility of the app layout / route guard.

---

## UI Structure

```
┌──────────────────────────────────────┐
│        Welcome to Veronex            │ ← CardTitle
│  Create your super admin account…   │ ← CardDescription
├──────────────────────────────────────┤
│  Username     [________________]     │
│  Password     [________________]     │
│  Confirm Pwd  [________________]     │
│                                      │
│  [error message if any]              │
│                                      │
│  [      Create account       ]       │ ← full-width Button; disabled while loading
└──────────────────────────────────────┘
```

- Centered card (`max-w-sm`) on a full-viewport background.
- No navigation bar — standalone layout (outside authenticated nav shell).
- Submit button label: `"Creating account…"` while `loading`, `"Create account"` otherwise.
- `autoComplete="username"` on username field; `autoComplete="new-password"` on password fields.

---

## No i18n

The setup page uses **hardcoded English strings** (no `useTranslation()` hook). All labels, card title, card description, error messages, and button text are inline string literals.

Hardcoded strings:
- Card title: `"Welcome to Veronex"`
- Card description: `"Create your super admin account to get started."`
- Error (short password): `"Password must be at least 8 characters"`
- Error (mismatch): `"Passwords do not match"`
- Error (409): `"Setup already completed. Please sign in."`
- Error (other): `"Setup failed. Please try again."`
- Button idle: `"Create account"`
- Button loading: `"Creating account…"`

---

## Related Docs

- Auth token storage and refresh: `../../auth/jwt-sessions.md`
- Login page (post-setup flow): `web/app/login/page.tsx`
- API auth handler: `../../auth/jwt-sessions.md`
