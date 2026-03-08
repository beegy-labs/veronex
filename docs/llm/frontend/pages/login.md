# Web — Login Page (/login)

> SSOT | Tier 2 | **Last Updated**: 2026-03-04

## Task Guide

| Task | Files |
|------|-------|
| Add form field | `web/app/login/page.tsx` form + `web/lib/api.ts` `login()` |
| Change remember-me behavior | `web/app/login/page.tsx` cookie helpers |
| Add OAuth / SSO button | `web/app/login/page.tsx` below form |
| Change locale options | `web/i18n/config.ts` `locales` + `localeLabels` |

## Overview

JWT credential login page. Username/password form that calls `POST /v1/auth/login`, stores tokens, and redirects to `/`. Includes theme toggle, locale selector, and optional username persistence via cookie.

## Key Files

| File | Purpose |
|------|---------|
| `web/app/login/page.tsx` | Login form, theme toggle, locale selector |
| `web/lib/api.ts` | `api.login()` — `POST /v1/auth/login` |
| `web/lib/auth.ts` | `setTokens()` — stores JWT access/refresh tokens |
| `web/lib/auth-guard.ts` | Redirect to `/login` when unauthenticated |
| `web/lib/types.ts` | `LoginRequest`, `LoginResponse` |
| `web/i18n/config.ts` | Locale list + labels |

## Page Layout

```
┌─────────────────────────┐
│  Login                  │
│  Description            │
│                         │
│  Username [________]    │
│  Password [________]    │
│  ☐ Remember username    │
│                         │
│  [    Sign In     ]     │
│                         │
│  🌙         [English v] │
└─────────────────────────┘
```

- Centered `Card` (`max-w-sm`) on full-viewport background
- Error message shown inline below checkbox on auth failure

## Features

### Remember Username

Persists username in a cookie (`veronex_saved_username`) with 30-day expiry. Pre-fills the username field on next visit. Cookie-based (not `localStorage`) to survive incognito sessions.

### Theme Toggle

`useTheme().toggleTheme` — dark/light mode via `Sun`/`Moon` icon button in card footer.

### Locale Selector

`<Select>` in card footer. Changes `i18next` language + persists to `localStorage`. Auto-detects browser locale on first visit.

## API Endpoint

| Method | Path | Auth | Body |
|--------|------|------|------|
| `POST` | `/v1/auth/login` | Public | `{ username, password }` |

Response: `LoginResponse` (access_token, refresh_token, username, role, account_id).

## i18n Keys

`auth.*`: login, loginDescription, username, password, rememberUsername, signingIn, invalidCredentials

Shared: `common.switchToLight`, `common.switchToDark`

## Related Docs

- JWT session model: `../../auth/jwt-sessions.md`
- Auth guard: `web/lib/auth-guard.ts`
