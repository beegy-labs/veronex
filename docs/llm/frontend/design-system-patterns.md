# Web — Next.js & React Patterns

> SSOT | **Last Updated**: 2026-03-25
> Core design system: `frontend/design-system.md`

---

## Next.js 16.2 / React 19.2

### `<Activity>` — State-Preserving Hide/Show

Replaces `{condition && <Component />}` when component must retain state across hide/show:

```tsx
import { Activity } from 'react'

// tab panels, collapsible sections, back-navigation preserved state
<Activity mode={isVisible ? 'visible' : 'hidden'}>
  <ExpensivePanel />
</Activity>
```

When hidden: effects unmount, updates deferred. State survives. Use instead of conditional render when remounting is expensive or state loss is unacceptable.

### `unstable_retry()` in error.tsx

Prefer `unstable_retry()` over `reset()` for data-fetching errors — does `router.refresh()` + `reset()` inside a transition:

```tsx
export default function Error({ reset, retry }: { reset: () => void; retry: () => void }) {
  return <Button onClick={retry}>{t('common.retry')}</Button>
}
```

### `useId` Prefix (React 19.2)

`useId()` now emits IDs with prefix `_r_` (was `:r:` in 19.0). Update snapshot tests or DOM assertions that check `useId` output:

```tsx
const safeId = rawId.replace(/_/g, '') // React 19.2: IDs are "_r0_" format
```

### Next.js 16.2 — No Mandatory Code Changes

Safe bump from 16.1.6. New opt-in flags (all off by default):
- `experimental.prefetchInlining` — reduces prefetch requests per link
- `experimental.appNewScrollHandler` — improved post-navigation focus

RSC payload deserialization is ~350% faster in 16.2 (zero config gain).

---

## Duration / Latency Formatter — `fmtMs` (SSOT)

Shared in `web/lib/chart-theme.ts`. Use everywhere — never inline ms conversion.

| Function | Use case | Example |
|----------|----------|---------|
| `fmtMs(n)` | KPI cards, tooltips, table cells | `86360` → `"1m 26s"` |
| `fmtMsAxis(n)` | Chart Y-axis tick labels | `86360` → `"1.4m"` |
| `fmtMsNullable(n)` | Nullable job latency | `null` → `"--"` |

Tiers: `< 1s` → `"Xms"` / `1s-59s` → `"X.Xs"` / `1m-59m` → `"Xm Xs"` / `>= 1h` → `"Xh Xm"`.
