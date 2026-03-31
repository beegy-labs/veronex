# Design System: Component Patterns

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> Provider taxonomy, network flow visualization, accounts page, dialogs, hooks, and registration flows.
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
7. Extend `PROVIDER_COLORS` map in Usage page
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
| `web/app/overview/components/live-feed.tsx` | Active jobs panel — React Query polls `GET /v1/dashboard/jobs?status=pending,running&limit=50` every 2s from DB |
| `web/hooks/use-inference-stream.ts` | SSE stream (`/v1/dashboard/jobs/stream`) — FlowEvents + FlowStats |

### Bee Particle Animation

Engine: CSS Motion Path (`offset-path`) + `@keyframes bee-fly` in `globals.css`. CSS is GPU-composited (2026 best practice over SVG SMIL). Fixed 540x264 logical space scaled via `ResizeObserver`. State managed by `useReducer` (SPAWN/EXPIRE actions); cleanup via `onAnimationEnd` (no setTimeout leaks). Max 30 concurrent bees. Enqueue color: `tokens.status.warning` (amber). Response bees: `JOB_STATUS_COLORS[status]` with `color-mix()` alpha overlay. `providerStroke([])` returns `tokens.border.subtle` (neutral when loading, not error).

**Dynamic bee sizing**: Particle diameter scales with traffic volume via CSS var `--bee-size`. `beeSize(count)` maps count 0-20+ to 6-18px (`Math.min(6 + Math.floor(count * 0.6), 18)`). Enqueue bees use `recentRequests*10`, dispatch/response bees use `runningJobs` as the count input.

### SVG Topology (540x264)

3-column ArgoCD-style layout, max-width 680px: Veronex API (Rect, cx=72) -> Queue/Valkey (Cylinder, cx=244) -> Ollama (Octagon, cx=460 cy=72) / Gemini (Octagon, cx=460 cy=192). Response arcs bypass Queue. See [pages/jobs.md](pages/jobs.md) for full path coordinates and phase details.

---

## Accounts Page (`web/app/accounts/page.tsx`)

Two-tab layout gated by `hasPermission('role_manage')`:

| Tab | Visible when | Content |
|-----|-------------|---------|
| Accounts | Always (page guard: `accounts` menu) | Account table with CRUD, role assignment, sessions, reset links |
| Roles | `hasPermission('role_manage')` | Role cards with permission checkboxes, menu checkboxes |

### Role Cards

Each role renders as a Card with permission badges. System roles (`is_system=true`) show a "System" badge and disable edit/delete buttons.

### Multi-Role Assignment

- Create account modal: checkbox list of all roles (default: viewer)
- Edit roles modal: checkbox list, N:N assignment via `api.updateAccount(id, { role_ids })`
- `account.roles` array displayed as Badge list in the accounts table

### Permission Checkboxes

`RoleEditorModal` renders all `ALL_PERMISSIONS` (including `role_manage`) as a 2-column checkbox grid. System roles have all checkboxes disabled.

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

---

## NavigationProgressProvider

File: `web/components/nav-progress.tsx`

Honeycomb-themed loading bar shown during page navigation and initial data fetches. Singleton — mounted once inside `AppShell` in `app/layout.tsx` (authenticated routes only).

```tsx
// app/layout.tsx — inside AppShell, after QueryClientProvider
return (
  <NavigationProgressProvider>
    <div className="flex h-full min-h-screen">
      <Nav />
      <main>{children}</main>
    </div>
  </NavigationProgressProvider>
)
```

| Trigger | Mechanism |
|---------|-----------|
| Page navigation (click) | `document` click listener on `<a>` tags |
| Page navigation (programmatic) | `usePathname()` change detection |
| Initial data fetch | `queryCache.subscribe` — only queries with `dataUpdatedAt === 0` and `status !== 'error'` |

**Key implementation rules:**
- `useProgressMachine()` tracks an active-source count (`countRef`) — `start()` increments, `finish()` decrements; bar only completes when count reaches 0
- `finish()` guards with `if (countRef.current <= 0) return` to ignore spurious calls
- `done()` force-completes the bar by resetting `countRef` to 0 — called on pathname change to prevent stale query start/finish pairs from keeping the bar visible indefinitely
- `reset()` is called on programmatic navigation to clear stale `pendingQueriesRef` entries
- `HoneycombBar` is wrapped with `React.memo` — props update at 80ms intervals during crawl
- SVG pattern IDs use `useId()` with `:` stripped (React IDs are not valid XML NCNames)
- Colors: track layer → `text-border`, fill layer → `text-primary`, glow → `tokens.brand.primary` via `color-mix()`

---

## 2-Step Verify Flow (Registration Modals)

Pattern for modals that register external services (GPU servers, Ollama providers). Requires connection verification before the register button becomes active.

**Shared type**: `VerifyState = 'idle' | 'checking' | 'ok' | 'error'` — exported from `web/lib/types.ts`.

```tsx
const [verifyState, setVerifyState] = useState<VerifyState>('idle')
const [verifyError, setVerifyError] = useState('')
const [verifiedUrl, setVerifiedUrl] = useState('')

const handleUrlChange = (val: string) => {
  setUrl(val)
  if (verifyState !== 'idle') { setVerifyState('idle'); setVerifyError('') }
}

const verifyMutation = useMutation({
  mutationFn: () => api.verifyServer(url.trim()),
  onSuccess: () => { setVerifyState('ok'); setVerifiedUrl(url.trim()) },
  onError: (e) => {
    setVerifyState('error')
    setVerifyError(
      e instanceof ApiHttpError && e.status === 409
        ? t('providers.servers.duplicateUrl')
        : (e instanceof Error ? e.message : t('providers.servers.connectionFailed'))
    )
  },
})

const isVerified = verifyState === 'ok' && url.trim() === verifiedUrl
const canRegister = !!name.trim() && isVerified && !registerMutation.isPending
```

**Rules:**
- URL change must reset verify state (`handleUrlChange`)
- Register button disabled unless `verifyState === 'ok'` AND URL hasn't changed since verification
- Backend independently re-validates on actual registration (defense-in-depth)
- 409 errors → `duplicateUrl` i18n key; 5xx errors → backend message; network errors → `connectionFailed` key
- i18n keys: `verifyConnection`, `verifying`, `connected`, `connectionFailed`, `duplicateUrl`, `verifyFirst` (add to all 3 locales)
