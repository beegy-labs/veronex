# Frontend Patterns — 4-Layer Architecture, Pages, Prefetch

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## Query Prefetch in AppShell

Queries depended on by multiple pages (e.g. `serversQuery` drives the dashboard waterfall) should be prefetched in `AppShell` on mount so they are cache-warm before the user navigates.

```tsx
// web/app/layout.tsx — AppShell
const queryClient = useQueryClient()
useEffect(() => {
  if (!isLoginPage && !isSetupPage && isLoggedIn()) {
    queryClient.prefetchQuery(serversQuery)
  }
  // isLoggedIn() is a pure synchronous cookie read — not React state, omit from deps
  // eslint-disable-next-line react-hooks/exhaustive-deps
}, [queryClient, isLoginPage, isSetupPage])
```

**Rule**: only prefetch queries that are universally needed across authenticated pages. Page-specific queries stay in the page component.

## Historical Data — `STALE_TIME_HISTORY`

Long-window historical queries (e.g. 60-day power/metrics history) use `STALE_TIME_HISTORY` (30 minutes).
Background refetch still runs on `REFETCH_INTERVAL_HISTORY` (5 minutes) to keep data fresh,
but re-navigation within 30 minutes skips the on-mount fetch and returns cached data immediately.

```typescript
// web/lib/queries/servers.ts
export const serverMetricsHistoryQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics-history', serverId],
  queryFn: () => api.serverMetricsHistory(serverId),
  staleTime: STALE_TIME_HISTORY,           // 30 min — re-nav returns cache instantly
  refetchInterval: REFETCH_INTERVAL_HISTORY, // 5 min — background refresh continues
})
```

**Rule**: `staleTime` and `refetchInterval` should reflect how quickly data actually changes, not be set to "slightly less than refetch". Use `STALE_TIME_HISTORY` for any query whose data window spans days or weeks.

---

## Page Guard (`usePageGuard`)

Permission-based access control at page level. Redirects to `/overview` if user lacks the required permission. Super-admin bypasses all checks. The permission supplied here MUST match the strictest `Require<Permission>` extractor on the page's API endpoints — the `web/lib/route-permissions.ts` SSOT and the regression test at `web/lib/__tests__/route-permissions.test.ts` enforce this.

```typescript
// web/hooks/use-page-guard.ts
export function usePageGuard(permission: Permission): void
// Usage: usePageGuard('audit_view') at top of page component
```

`hasPermission()` reads from JWT claims `permissions` array (merged across all roles assigned to the account). Nav items in `web/components/nav.tsx` declare `permission: Permission` and are filtered by the same predicate, so a sidebar entry never appears for a page whose API will return 403.

---

## Adding a New Page

```
1. web/lib/types.ts            -- add TypeScript types (+ Zod schema if untrusted data)
2. web/lib/api.ts              -- add API functions to the api object
3. web/lib/queries/domain.ts   -- add queryOptions factory (SSOT for queryKey + staleTime)
4. web/app/new-page/page.tsx   -- 'use client' + useQuery(domainQuery) + UI
5. web/components/nav.tsx      -- add navItems entry
6. web/messages/en.json        -- add i18n keys (source of truth)
7. web/messages/ko.json        -- Korean translation
8. web/messages/ja.json        -- Japanese translation
9. docs/llm/frontend/pages/*   -- update CDD doc
```

---

## 4-Layer Component Architecture

| Layer | Path | Rule |
|-------|------|------|
| 1. Pages | `app/*/page.tsx` | Route entry — `useQuery` wiring + layout only |
| 2. Feature components | `app/*/components/` | Page-specific composed UI — not shared |
| 3. Shared components | `components/` + `components/ui/` | Reusable across pages — no business logic |
| 4. Foundation | `lib/` · `hooks/` · `lib/queries/` | Types, API, formatters, tokens, query factories |

### Violations (all are P1)

| Violation | Fix |
|-----------|-----|
| Shared logic in feature dirs | Extract to `components/` or `lib/` |
| Page-specific logic in `components/` | Move to `app/{route}/components/` |
| Cross-route import (`app/A/` imports `app/B/components/`) | Lift shared dep to `components/` or duplicate per route |
| **Single-importer shared component** (file in `components/` imported by exactly one route) | Move down to `app/{route}/components/` |

### Non-Goals (do not propose these)

Atomic Design is **explicitly rejected** for this codebase:

| Rejected | Reason |
|----------|--------|
| `components/atoms/`, `components/molecules/`, `components/organisms/`, `components/templates/` | Conflicts with App Router colocation; Vercel + shadcn/ui 2026 standard is `components/ui/` primitives + `app/{route}/components/` feature folders |
| Using terms "atom/molecule/organism" in PR reviews, commits, or docs | Classification is ambiguous (is a `Button` atom or molecule?) — produces unproductive boundary debates |
| Global `organisms/` folder | Organisms are typically route-specific → global location creates orphan files on route deletion and blurs ownership |
| Renaming 4-Layer terminology to Atomic terms | Training data overwhelmingly uses `app/route/components/` pattern → keeps LLM generation accuracy high |

If new structure is needed, extend 4-Layer (add sub-folders like `app/{route}/components/modals/`), do not introduce a parallel taxonomy.

---

