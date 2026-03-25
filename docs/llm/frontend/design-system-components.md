# Web -- Component Patterns & Auth Architecture

> SSOT | **Last Updated**: 2026-03-21 | Split from design-system.md

Related files:
- [design-system.md](design-system.md) -- brand, tokens, theme, nav, DataTable, state management
- [design-system-i18n.md](design-system-i18n.md) -- i18n, timezone, date formatting

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new provider type | See "Adding a New Provider" section below | 5-step process |
| Add public (no-auth) route | `web/lib/auth-guard.ts` `PUBLIC_PATHS` array | Prevents reload loop on unauthenticated pages |
| Change auth cookie expiry | `web/lib/auth.ts` cookie settings | Currently 7 days, `SameSite=Strict` |
| Change refresh mutex behavior | `web/lib/auth-guard.ts` `tryRefresh()` | Module-level state, survives re-renders |
| Add new flow visualization panel | `web/app/overview/components/` | Create panel + update `network-flow-tab.tsx` |

---

## Shared Components (SSOT for extraction)

Components designed for reuse across Veronex and future projects (veronex-ai, etc).

### Primitives (`web/components/ui/`)

Radix-based, unstyled. All use `--theme-*` CSS tokens.

| Component | File | Notes |
|-----------|------|-------|
| Badge | `badge.tsx` | Always add `whitespace-nowrap` for i18n text |
| Button | `button.tsx` | Variants: default, secondary, outline, ghost, destructive |
| Card | `card.tsx` | CardHeader, CardContent, CardTitle, CardDescription |
| Checkbox | `checkbox.tsx` | |
| Dialog | `dialog.tsx` | Modal with overlay |
| Input | `input.tsx` | |
| Label | `label.tsx` | |
| Select | `select.tsx` | Radix Select with trigger, content, item |
| Separator | `separator.tsx` | |
| Switch | `switch.tsx` | |
| Table | `table.tsx` | TableHeader, TableBody, TableRow, TableHead, TableCell |
| Tabs | `tabs.tsx` | TabsList, TabsTrigger, TabsContent |
| Tooltip | `tooltip.tsx` | TooltipProvider, TooltipTrigger, TooltipContent |

### Domain Components (`web/components/`)

| Component | File | Purpose | Reusable? |
|-----------|------|---------|-----------|
| StatusPill | `status-pill.tsx` | Count pill with icon + label. Optional `count`. `whitespace-nowrap` built-in. | Yes |
| StatusBadge | `providers/shared.tsx` | Online/Offline/Degraded badge with icon | Yes |
| DataTable | `data-table.tsx` | Scrollable table wrapper with `minWidth` + optional footer | Yes |
| TimeRangeSelector | `time-range-selector.tsx` | Preset buttons (1h/6h/24h/7d/30d) + custom date range (calendar) | Yes |
| StatsCard | `stats-card.tsx` | KPI card with title, value, subtitle, icon | Yes |
| ProgressBar | `progress-bar.tsx` | Colored progress bar with percentage | Yes |
| SectionLabel | `section-label.tsx` | Section header text | Yes |
| ConfirmDialog | `confirm-dialog.tsx` | Delete/action confirmation | Yes |
| CopyButton | `copy-button.tsx` | Clipboard copy with feedback | Yes |

### Design Tokens (`web/lib/design-tokens.ts`)

Single source for all programmatic color references (charts, dynamic styles).
CSS custom properties defined in `web/styles/tokens.css`.

### Extraction Guide

To extract shared components into a separate package:
1. Move `web/components/ui/` → `packages/ui/`
2. Move domain components (StatusPill, DataTable, TimeRangeSelector, etc.) → `packages/veronex-ui/`
3. Keep `web/lib/design-tokens.ts` and `web/styles/tokens.css` in the UI package
4. Import via workspace alias: `@veronex/ui`

---

## Login Page (`web/app/login/page.tsx`)

Public route (`/login`) -- no auth required.

| Feature | Implementation |
|---------|----------------|
| Remember username | Checkbox -- saves to `veronex_saved_username` cookie (30-day, `SameSite=Lax`); pre-fills on mount |
| Language selector | Compact `<Select>` in card footer (en / ko / ja) -- calls `i18n.changeLanguage()` + persists to `localStorage['hg-lang']` |
| i18n | All labels use `t('auth.*')` keys |

| Cookie | Value | Expiry | SameSite |
|--------|-------|--------|----------|
| `veronex_saved_username` | Saved username for pre-fill | 30 days | Lax |

This cookie is not a session token -- it only pre-fills the login form. Managed entirely in `login/page.tsx` (not in `auth.ts`).

---

## Auth -- Cookie-Based Session (`web/lib/auth.ts`)

Tokens stored in browser cookies (not localStorage) for persistence across restarts. All cookies: `SameSite=Strict; path=/; expires={7d}`.

| Cookie | Value |
|--------|-------|
| `veronex_access_token` | JWT access token |
| `veronex_refresh_token` | Refresh token |
| `veronex_username` | Display name |
| `veronex_role` | `super` or `admin` |
| `veronex_account_id` | UUIDv7 |

| Function | Purpose |
|----------|---------|
| `getAccessToken()` / `getRefreshToken()` | Read token cookies |
| `setTokens(LoginResponse)` | Write all 5 cookies |
| `setAccessToken(token)` | Update access token after refresh |
| `clearTokens()` | Delete all 5 on logout |
| `getAuthUser()` | Returns `{ username, role, accountId }` or null |
| `isLoggedIn()` | True when access_token present |

---

## Auth Guard -- SSOT for Token Lifecycle (`web/lib/auth-guard.ts`)

All auth flow policy lives here. Never duplicate refresh or redirect logic in other files.

| File | Owns |
|------|------|
| `auth.ts` | Token CRUD -- cookie read/write/clear only |
| `auth-guard.ts` | Auth flow SSOT -- mutex, refresh, redirect, public paths |
| `api-client.ts` | HTTP transport only -- delegates 401 to auth-guard |
| `nav.tsx` logout | Calls `redirectToLogin()` -- no manual `clearTokens()` + `window.location` |

### PUBLIC_PATHS

```typescript
export const PUBLIC_PATHS = ['/login', '/setup'] as const
export function isPublicPath(pathname: string): boolean { ... }
```

Add any unauthenticated route here. `redirectToLogin()` is a no-op on these paths, preventing the reload loop caused by `LabSettingsProvider` calling authenticated endpoints on pages that do not require login.

### Token Refresh Mutex

When multiple queries receive 401 simultaneously, the first caller creates `refreshMutex = doRefresh()`. All subsequent callers piggyback on the same Promise. A single `POST /v1/auth/refresh` is sent. On success, `setAccessToken()` updates the cookie, `.finally()` clears the mutex, and all callers retry with the new token.

```typescript
let refreshMutex: Promise<boolean> | null = null
let redirecting = false

export function tryRefresh(): Promise<boolean> {
  if (refreshMutex !== null) return refreshMutex
  refreshMutex = doRefresh().finally(() => { refreshMutex = null })
  return refreshMutex
}

export function redirectToLogin(): void {
  if (redirecting) return
  if (isPublicPath(window.location.pathname)) return
  redirecting = true
  clearTokens()
  window.location.href = '/login'
}
```

Module-level state (not class-level) survives re-renders and is shared across all callers.

---

## Status Colors and Indicators

Follow Carbon Design System 3-element rule: **color + icon + text** (never color alone -- WCAG 1.4.1).

| Status | Tailwind class | Icon | Text key |
|--------|----------------|------|----------|
| Connected | `text-status-success-fg` | filled dot | `overview.connected` |
| Unreachable | `text-status-error-fg` | filled dot | `overview.unreachable` |
| Normal (<80C) | `text-status-success-fg` | `CheckCircle2` | `overview.tempNormal` |
| Warning (80-89C) | `text-status-warning-fg` | `AlertTriangle` | `overview.tempWarning` |
| Critical (>=90C) | `text-status-error-fg` | `XCircle` | `overview.tempCritical` |

### Job Status Colors

All job status colors live in `JOB_STATUS_COLORS` in `web/lib/constants.ts` (uses `tokens.*` internally).

| Status | `tokens.*` key | Tailwind class |
|--------|---------------|----------------|
| `pending` | `tokens.status.warning` | `text-status-warning-fg` |
| `running` | `tokens.status.info` | `text-status-info-fg` |
| `completed` | `tokens.status.success` | `text-status-success-fg` |
| `failed` | `tokens.status.error` | `text-status-error-fg` |
| `cancelled` | `tokens.status.cancelled` | `text-muted-foreground` |

### Provider Status Colors

Provider `StatusBadge`: `online` = `tokens.status.success`, `degraded` = `tokens.status.warning`, `offline` = `tokens.text.faint`.
All mappings in `PROVIDER_STATUS_DOT` / `PROVIDER_STATUS_BADGE` / `PROVIDER_STATUS_TEXT` in `web/lib/constants.ts`.

### Provider/Finish Colors (Usage page SSOT)

```ts
import { tokens } from '@/lib/design-tokens'

// From web/lib/constants.ts — already use tokens internally
const PROVIDER_COLORS = { ollama: tokens.brand.primary, gemini: tokens.status.info }
const FINISH_COLORS   = {
  stop: tokens.status.success, length: tokens.status.warning,
  error: tokens.status.error,  cancelled: tokens.text.secondary,
}
```

Extend these maps when adding new provider types — never hardcode provider names in JSX. Note: keys match `provider_type` field values.

---

→ Provider taxonomy, network flow, dialogs, hooks: `design-system-components-patterns.md`
