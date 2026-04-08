# Next.js 15/16 — Breaking Changes

> **Last Researched**: 2026-04-07 | **Source**: Next.js 15/16 official blog + web search
> **Companion**: `research/frontend/nextjs.md` — core patterns

---

## Caching Defaults Changed (Next.js 15)

| Behavior | Next.js 14 | Next.js 15 |
|----------|------------|------------|
| `fetch()` default | `force-cache` (cached) | `no-store` (uncached) |
| GET Route Handlers | Cached | Not cached |
| Client Router Cache (pages) | 30s stale | 0s (always fresh) |

To restore v14 caching: `experimental.staleTimes: { dynamic: 30 }` in `next.config.ts`.

---

## Async Request APIs (Breaking)

`params`, `searchParams`, `cookies()`, `headers()` are now Promises:

```ts
// Next.js 15 — must await
export default async function Page({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  return <h1>{slug}</h1>;
}

// Client Component — use React.use()
'use client';
function Page({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = use(params);
  return <h1>{slug}</h1>;
}
```

Auto-migrate: `npx @next/codemod@canary next-async-request-api .`

---

## New Stable Features (15/16)

| Feature | Detail |
|---------|--------|
| Turbopack dev | Stable — `next dev --turbo`. 76.7% faster startup, 96.3% faster Fast Refresh |
| `next.config.ts` | TypeScript-native config with full type inference via `NextConfig` |
| `<Form>` component | `next/form` — prefetch + client navigation for search forms |
| `instrumentation.js` | Stable — `onRequestError()` for observability |
| PPR | Stable in Next.js 16 (`cacheComponents: true` in config) |

---

## `use cache` Directive (Next.js 16 stable)

Replaces v14's implicit caching with explicit opt-in:

```ts
// next.config.ts
const nextConfig = { experimental: { dynamicIO: true } };

export async function getProducts() {
  'use cache';
  cacheLife('hours');   // profiles: seconds/minutes/hours/days/weeks
  cacheTag('products'); // for on-demand invalidation
  return db.query('...');
}

// Invalidate from Server Action
export async function updateProduct() {
  'use server';
  await db.update(...);
  revalidateTag('products');
}
```

**This codebase:** Not applicable — all pages are `'use client'` (TanStack Query). Architecture deliberately avoids Next.js data caching in favor of TanStack Query's client-side cache.

---

## Sources

- [Next.js 15 Official Blog](https://nextjs.org/blog/next-15)
- [Next.js 15 breaking cache changes](https://medium.com/@weijunext/next-js-15-introduces-breaking-cache-strategy-changes-a594e3b504df)
- [Params as Promise in Next.js 15](https://medium.com/@ayonaalex2/params-search-params-resolved-as-promise-in-next-js-15-444317307481)
