# Next.js 16 — Research

> **Last Researched**: 2026-03-02 | **Source**: Implementation experience + web search
> **Status**: ✅ Verified — all patterns researched and documented

---

## Confirmed Patterns (this codebase)

### App Router Structure

```
web/app/
├── layout.tsx          # Root layout (fonts, providers, QueryClientProvider)
├── globals.css         # Tailwind v4 entry + global CSS
├── tokens.css          # Design token SSOT (@property / --palette-* / --theme-* / @theme)
├── overview/
│   ├── page.tsx        # Page component ('use client')
│   └── components/     # Co-located components
├── jobs/page.tsx
├── keys/page.tsx
...
```

- All pages are `'use client'` (TanStack Query + interactivity)
- Co-locate page-specific components in `page-dir/components/`
- Shared components in `web/components/`

---

### `'use client'` Boundary

```tsx
// All data-fetching pages use 'use client'
'use client'

import { useQuery } from '@tanstack/react-query'
```

TanStack Query requires client context. Since all pages fetch data, they are all client components.
Server Components are not used in this codebase (as of 2026-03-01).

---

### PostCSS + Tailwind v4

```
postcss.config.mjs → @tailwindcss/postcss
```

No `tailwind.config.ts`. All customization is in CSS via `@theme` and `@property`.

```css
/* tokens.css — Tailwind v4 CSS-first approach */
@theme inline {
  --color-primary: var(--theme-primary);
  --color-background: var(--theme-bg-base);
  ...
}
```

---

### i18n (next-intl or custom)

Custom `useTranslation` hook, not `next-intl`. Messages in `web/messages/{locale}.json`.
Locale files: `en.json`, `ko.json`, `ja.json`.

---

---

## Server Components vs Client Components (Next.js 16)

### General Rule (2026)

Default to **Server Components** — they render on the server, ship zero client JS, and can `await` directly in JSX. Add `'use client'` only at the leaf boundary where interactivity is needed.

```
Page (Server) → Layout (Server) → [DataFetcher (Server)] → [InteractiveWidget (Client)]
                                                                        ↑
                                                              smallest 'use client' boundary
```

### This Codebase: All Pages Are `'use client'` (Intentional)

**Why:** TanStack Query v5 requires `QueryClientProvider`, which is a Client Component context provider. Since all 13 pages poll live data, keeping them as Client Components is the correct architectural choice for this admin dashboard.

This is NOT a compromise — it is the right pattern for:
- Real-time polling dashboards
- Optimistic mutations with rollback
- Persistent cache across navigations
- Background refetch when tab becomes visible

**Server Components are appropriate when:** content is mostly static, data can be co-located with the rendering, and no polling is needed.

---

## Server Actions vs API Routes (2026)

| | Server Actions | API Routes (`/api/`) |
|---|---|---|
| Use for | Form mutations, CRUD from components | Public APIs, webhooks, external integrations |
| Overhead | No HTTP round-trip, no JSON serialization | Full HTTP request |
| Latency improvement | ~15-30% faster than API route | Baseline |
| Auth | Inherits session from server render | Requires explicit auth middleware |
| `revalidatePath` | Yes, called inline after mutation | Via `fetch` + manual cache bust |

**This codebase uses API routes** (not Server Actions) because:
1. The backend is a separate Rust service — there is no "database direct access" from Next.js
2. JWT auth is managed in `apiClient` (client-side token storage + refresh)
3. All mutations go through `POST/PATCH/DELETE` to the Rust API

Server Actions would add value only if Next.js had its own DB layer, which it does not.

---

## Partial Pre-Rendering (PPR — Next.js 16)

PPR statically renders the page shell and streams dynamic slots — combining static and dynamic in one request without an extra network hop.

```tsx
// app/page.tsx (PPR-enabled page)
import { Suspense } from 'react'

export default function Page() {
  return (
    <main>
      <StaticHeader />        {/* served from edge cache immediately */}
      <Suspense fallback={<Skeleton />}>
        <DynamicFeed />       {/* streamed in after static shell */}
      </Suspense>
    </main>
  )
}
```

**Verdict for this codebase:** PPR is not applicable because:
- All pages are `'use client'` (PPR requires Server Components for the static shell)
- Data is fully dynamic (no page would benefit from edge caching)
- **Do not add PPR** unless a page has a meaningful static shell with isolated dynamic slots

---

## `<Suspense>` with React 19 `use()` Hook

The `use()` hook (React 19) unwraps a Promise inside a Client Component and suspends until resolved:

```tsx
// Server Component: creates a Promise and passes it down
async function ServerPage() {
  const dataPromise = fetchData()           // NOT awaited
  return <ClientList dataPromise={dataPromise} />
}

// Client Component: receives Promise, use() suspends it
'use client'
function ClientList({ dataPromise }: { dataPromise: Promise<Item[]> }) {
  const data = use(dataPromise)             // suspends here
  return <ul>{data.map(d => <li key={d.id}>{d.name}</li>)}</ul>
}

// Wrap with Suspense in the parent
<Suspense fallback={<Skeleton />}>
  <ClientList dataPromise={promise} />
</Suspense>
```

**Verdict for this codebase:** Not used because all pages are client-only (TanStack Query polling). TanStack Query handles its own loading/error states. Use `<Suspense>` only if Server Component streaming is adopted in the future.

---

## Loading States: TanStack Query vs Suspense

| Pattern | When to use |
|---------|-------------|
| `isLoading` / `isPending` guard | Client Component polling (current codebase pattern) |
| `<Suspense fallback>` | Server Component streaming or `use()` hook in Client Components |
| `isLoading && <Skeleton>` | Fine for per-component loading UX without Suspense |

**Current codebase uses `isLoading` guards** — correct for TanStack Query polling architecture.

---

## Sources

- Next.js 16 docs: https://nextjs.org/docs
- Verified: `web/app/` directory structure, `web/app/layout.tsx`
- Web search: Next.js 16 App Router 2026 patterns, PPR, Server Actions
