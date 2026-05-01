# Frontend Execution Contracts

> LLM-enforced domain contracts | **Last Updated**: 2026-05-01

## 0. HTML Cache-Control Override (build-output policy)

`web/next.config.ts` ships an `async headers()` override that sets
`Cache-Control: no-store, must-revalidate` on every response that is NOT
a content-hashed static asset (matches `^/((?!_next/static|_next/image|favicon).*)`).

**Why this exists**: Default Next.js prerender output emits
`Cache-Control: s-maxage=31536000` on the HTML, which Cloudflare honors
as a 1-year edge cache. Each new deploy lands in the cluster but the old
HTML keeps pointing browsers at old chunk-hash filenames — every web fix
goes invisible until the edge cache is manually purged. Verified live
2026-05-01 against `https://veronex-dev.verobee.com/jobs` (`x-nextjs-cache:
HIT`, `cache-control: s-maxage=31536000`, multi-PR fix appeared deployed
at the pod but did not reach users until manual CF purge).

**Do not remove**: removing this override re-introduces the silent-stale
deploy class. JS/CSS bundles under `/_next/static/*` are content-hashed
and immutable — they're explicitly excluded from the rule and continue
to use Next.js's own long-cache headers. Net cost is ~5–50ms per
navigation for the HTML fetch from origin; for an auth-gated SaaS where
every page is user-specific, prerendered-static caching never had value.

## 1. Feature Folder Structure

```
web/
├── app/
│   └── {route}/
│       ├── page.tsx                  # Route entry — useQuery wiring + layout only
│       ├── layout.tsx                # Route layout (optional)
│       └── components/               # Feature-scoped components (NOT shared)
│           ├── {FeatureName}Card.tsx
│           ├── {FeatureName}Table.tsx
│           └── {FeatureName}Dialog.tsx
├── components/                       # Shared across 2+ pages
│   └── ui/                           # Primitive UI components
├── lib/
│   ├── api.ts                        # HTTP SSOT — all API calls
│   ├── types.ts                      # TypeScript types + Zod schemas
│   ├── queries/
│   │   └── {domain}.ts               # queryOptions factory SSOT
│   ├── design-tokens.ts
│   └── chart-theme.ts
└── hooks/
    └── use{Name}.ts                  # Shared hooks only
```

### Rules

- `app/{route}/components/` — feature-scoped only; never import across routes
- `components/` — shared only; must be used in 2+ routes before placing here
- `lib/queries/{domain}.ts` — one file per API domain; all queryOptions for that domain
- Never create `utils/` inside a feature folder — shared utils go in `lib/`
- Never create a `store/` or `context/` at the feature level

---

## 2. State Classification

| State Type | Location | Pattern |
|-----------|----------|---------|
| Server state | `lib/queries/{domain}.ts` + `useQuery` | `queryOptions()` factory |
| UI state (local) | Component `useState` | Never lifted unless 2+ siblings need it |
| UI state (shared) | Closest common ancestor `useState` | Prop drilling max 2 levels |
| Global app state | `lib/` module-level or React Context | Only for auth, theme, i18n |
| Realtime/stream | `useQuery` + `withJitter` polling or `experimental_streamedQuery` | Never raw `setInterval` |

### Rules

- Never copy `useQuery` data into `useState` — single source of truth
- Never `useEffect` to fetch data — use `useQuery`
- `useState` is for UI interaction state only (open/closed, selected tab, form input)
- Polling via `refetchInterval: withJitter(REFETCH_INTERVAL_FAST)` — never raw `setInterval`

---

## 3. Naming Conventions

### Files

| Type | Convention | Example |
|------|-----------|---------|
| Page | `page.tsx` | `app/services/[id]/page.tsx` |
| Feature component | `PascalCase.tsx` | `ServiceCard.tsx` |
| Query factory | `camelCase.ts` | `lib/queries/services.ts` |
| Hook | `useCamelCase.ts` | `hooks/useServiceId.ts` |
| Types | `PascalCase` in `lib/types.ts` | `ServiceDetail` |

### Query Keys

```typescript
// lib/queries/services.ts
export const SERVICE_QUERY_KEYS = {
  list:   (accountId: string)          => ['services', accountId] as const,
  detail: (accountId: string, id: string) => ['services', accountId, id] as const,
}
```

Rule: never inline `queryKey` arrays in components — always reference `QUERY_KEYS` constants.

---

## 4. Common Module Import Contract

All features MUST use these shared modules. Never reimplement.

| Module | Import | Provides |
|--------|--------|---------|
| HTTP client | `import { apiGet, apiPost, ... } from '@/lib/api'` | Authenticated fetch, 401 refresh |
| Error type | `import { ApiHttpError } from '@/lib/types'` | Typed HTTP errors |
| Query timing | `import { STALE_TIME_FAST, withJitter } from '@/lib/queries/constants'` | Consistent cache timing |
| Formatters | `import { fmtMs, fmtCompact, fmtPct } from '@/lib/chart-theme'` | Display formatting |
| Design tokens | `import { tokens } from '@/lib/design-tokens'` | Token-compliant styling |
| i18n | `import { useTranslations } from 'next-intl'` | All user-facing strings |

### Error Handling Contract

```typescript
// ✅ Correct
onError: (e) => {
  const msg = e instanceof ApiHttpError && e.status === 409
    ? t('error.conflict')
    : e instanceof Error ? e.message : t('error.unknown')
}

// ❌ Never
onError: (e: any) => { if (e.status === 409) ... }
```

### Realtime Contract

```typescript
// ✅ Correct — polling via TanStack Query
useQuery({
  ...serviceQuery(id),
  refetchInterval: withJitter(REFETCH_INTERVAL_FAST),
})

// ❌ Never
useEffect(() => {
  const id = setInterval(() => fetch(...), 5000)
  return () => clearInterval(id)
}, [])
```

---

## 5. Feature Boundary Rules

- Feature components (`app/{route}/components/`) must NOT import from another route's `components/`
- `lib/` and `components/` are the only cross-feature boundaries
- If logic is needed in 2+ features → move to `lib/` or `hooks/`
- If a component is needed in 2+ features → move to `components/`
- Never create circular imports between `lib/` modules

---

## Layer Violation Examples

```
// ❌ Business logic in shared component
// components/ServiceCard.tsx
const { data } = useQuery(serviceQuery(id))   // NO — page layer responsibility

// ❌ Feature component in wrong location
// components/ServiceDetailModal.tsx          // NO — used only in services route
// app/services/[id]/components/ServiceDetailModal.tsx  // ✅

// ❌ Inline queryKey
useQuery({ queryKey: ['services', id], queryFn: ... })  // NO

// ✅ queryOptions factory
useQuery(serviceDetailQuery(id))
```
