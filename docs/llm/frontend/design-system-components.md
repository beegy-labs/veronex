# Web -- Component Patterns & Auth Architecture

> SSOT | **Last Updated**: 2026-03-08 | Split from design-system.md

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

| Status | Color class | Icon | Text key |
|--------|-------------|------|----------|
| Connected | `text-status-success-fg` | filled dot | `overview.connected` |
| Unreachable | `text-status-error-fg` | filled dot | `overview.unreachable` |
| Normal (<80C) | `text-status-success-fg` | `CheckCircle2` | `overview.tempNormal` |
| Warning (80-89C) | `text-status-warning-fg` | `AlertTriangle` | `overview.tempWarning` |
| Critical (>=90C) | `text-status-error-fg` | `XCircle` | `overview.tempCritical` |

### Job Status Colors

| Status | Color token |
|--------|-------------|
| `pending` | `var(--theme-status-warning)` -- amber |
| `running` | `var(--theme-status-info)` -- blue |
| `completed` | `var(--theme-status-success)` -- green |
| `failed` | `var(--theme-status-error)` -- red |
| `cancelled` | `var(--theme-status-cancelled)` -- slate/gray |

### Provider Status Colors

Provider `StatusBadge`: `online` = success green, `degraded` = warning amber, `offline` = muted.

### Provider/Finish Colors (Usage page SSOT)

```ts
const BACKEND_COLORS = { ollama: 'var(--theme-primary)', gemini: 'var(--theme-status-info)' }
const FINISH_COLORS  = {
  stop: 'var(--theme-status-success)', length: 'var(--theme-status-warning)',
  error: 'var(--theme-status-error)', cancelled: 'var(--theme-text-secondary)',
}
```

Extend these maps when adding new provider types -- never hardcode provider names in JSX. Note: `BACKEND_COLORS` uses `provider_type` field values as keys.

---

## Provider Taxonomy (Dashboard)

Providers are grouped into two generic categories (future-proof):

| Category | i18n key | Icon | `provider_type` values |
|----------|----------|------|----------------------|
| Local | `overview.localProviders` | `Server` | `['ollama']` |
| API Services | `overview.apiProviders` | `Globe` | `['gemini']` |

Never hard-code "Ollama" or "Gemini" labels in Overview. Use `localProviders`/`apiProviders` i18n keys.

---

## Adding a New Provider (e.g. OpenAI)

1. Add entry to `navItems[].children` in `nav.tsx` (under `providers` group)
2. Add `section === 'openai'` branch in `providers/page.tsx` -> new `<OpenAITab>`
3. Add i18n key `nav.openai` + tab strings to all 3 message files
4. Extend `ProviderType` enum in Rust + add adapter in `infrastructure/outbound/`
5. Update `docs/llm/providers/` + `docs/llm/inference/openai-compat.md`
6. Create `docs/llm/frontend/pages/providers.md` section for the new tab
7. Extend `BACKEND_COLORS` map in Usage page
8. Add to provider taxonomy array in Dashboard tab

---

## Network Flow Visualization

Real-time inference traffic visualization. Accessible as the 3rd tab on `/jobs` page. Full documentation: [pages/jobs.md](pages/jobs.md).

### Component Architecture

| File | Role |
|------|------|
| `web/app/overview/components/network-flow-tab.tsx` | Composes ProviderFlowPanel + LiveFeed |
| `web/app/overview/components/provider-flow-panel.tsx` | SVG topology: API -> Queue -> Providers |
| `web/app/overview/components/dashboard-helpers.tsx` | Shared: ThermalBadge, ConnectionDot, ProviderRow |
| `web/app/overview/components/dashboard-lower-sections.tsx` | RequestTrend, TopModels, RecentJobs, TokenSummary |
| `web/app/overview/components/live-feed.tsx` | Scrollable real-time event list |
| `web/hooks/use-inference-stream.ts` | 5s TanStack Query polling |

### Bee Particle Animation

Engine: CSS Motion Path (`offset-path`) + `@keyframes bee-fly` in `globals.css`. CSS is GPU-composited (2026 best practice over SVG SMIL). Fixed 360x240 logical space scaled via `ResizeObserver`. State managed by `useReducer` (SPAWN/EXPIRE actions); cleanup via `onAnimationEnd` (no setTimeout leaks). Max 30 concurrent bees, `BEE_STAGGER_MS = 700` (half of 1400ms duration). Enqueue color: `#facc15` (yellow-400). Response bees dimmed: `color + 'cc'` bg, `color + '28'` glow.

### SVG Topology (540x264)

3-column ArgoCD-style layout, max-width 680px: Veronex API (Rect, cx=72) -> Queue/Valkey (Cylinder, cx=244) -> Ollama (Octagon, cx=460 cy=72) / Gemini (Octagon, cx=460 cy=192). Response arcs bypass Queue. See [pages/jobs.md](pages/jobs.md) for full path coordinates and phase details.

---

## ConfirmDialog

File: `web/components/confirm-dialog.tsx`

Reusable confirmation dialog for destructive actions (delete account, revoke key).

Props: `open`, `onClose`, `onConfirm`, `title`, `description`, `confirmLabel`, `isLoading`, `variant`

Usage:
```tsx
<ConfirmDialog
  open={!!deleteTarget}
  onClose={() => setDeleteTarget(null)}
  onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
  title={t('keys.deleteConfirm')}
  description={t('keys.deleteWarning')}
  confirmLabel={t('common.delete')}
  variant="destructive"
/>
```

---

## useApiMutation

File: `web/hooks/use-api-mutation.ts`

Wraps TanStack `useMutation` with automatic query invalidation.

```tsx
const deleteMutation = useApiMutation(
  (id: string) => api.deleteKey(id),
  { invalidateKey: ['keys'] }
);
```

Eliminates repeated `useQueryClient()` + `onSettled` invalidation boilerplate.
