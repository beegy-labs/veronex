# Frontend Patterns — React 19/19.2 + Compiler + Performance

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## React 19 -- useOptimistic

Apply optimistic updates to all toggle/switch mutations for perceived speed.

```typescript
import { useOptimistic } from 'react'

const [optimisticEnabled, setOptimistic] = useOptimistic(
  model.is_enabled,
  (_, newValue: boolean) => newValue
)

const mutation = useMutation({
  mutationFn: (v: boolean) => api.setModelEnabled(providerId, model.model_name, v),
  onError: () => setOptimistic(model.is_enabled),
})

<Switch
  checked={optimisticEnabled}
  onCheckedChange={(v) => { setOptimistic(v); mutation.mutate(v) }}
/>
// UI responds instantly -> server syncs in background -> reverts if error
```

---

## React Compiler (v1.0, October 2025)

React Compiler handles `useMemo`, `useCallback`, and `React.memo` automatically for the vast majority of components. **Do not add manual memoization unless one of these specific conditions applies:**

1. Third-party library requires reference equality the compiler can't satisfy (e.g., react-dnd, some animation libs)
2. Library uses interior mutability the compiler can't see through (e.g., react-hook-form `watch`)
3. Profiler-measured hot path the compiler's heuristics miss (SSE-driven, ≥1 update/sec)

Use `'use no memo'` directive to exclude a single component from compilation when debugging.

> See: `docs/llm/research/frontend/react.md § React Compiler`

## useMemo for Derived Data

> **2026:** React Compiler handles most derived state automatically. Write `useMemo` only for the exceptions listed in the React Compiler section above.

Wrap filter/sort/slice/map chains with `useMemo` when the input comes from query data and the compiler exception conditions apply:

```typescript
const modelBarData = useMemo(() =>
  (breakdown?.by_model ?? [])
    .filter(m => geminiEnabled || m.provider_type !== 'gemini')
    .sort((a, b) => b.request_count - a.request_count)
    .slice(0, 8)
    .map(m => ({ ...m, label: truncate(m.model_name, 22) })),
  [breakdown?.by_model, geminiEnabled],
)
```

Not needed for simple property access or single-value derivations.

## Performance Rules

| Rule | Detail |
|------|--------|
| Derived state | React Compiler handles automatically; manual `useMemo` only for compiler exceptions (see React Compiler section) |
| Handler refs | React Compiler handles automatically; manual `useCallback` only for compiler exceptions |
| Heavy panels | Modals/charts with conditional render → `dynamic(() => import(...), { ssr: false })` |
| Query dedup | Same `queryKey` in sibling components → lift to parent or share `queryOptions` factory |
| SSE-driven components | Props updated ≥1/s from SSE or `setInterval` ≤100ms → `React.memo` required (compiler exception #3) |
| Time-display staleness | Components showing relative time (e.g. "5s ago") → `setInterval` tick (10–30s) required |
| Zero-value stat containers | Stat rows showing counts from live data → hidden when all values are 0 |
| React key | Never use array `index` as sole key for reorderable lists |

---

## React 19.2 Patterns

### `<Activity>` — State-Preserving Conditional Render

Replaces conditional rendering patterns that lose component state on hide/show (e.g. tab panels, back-navigation preservation):

```tsx
import { Activity } from 'react'

// Replaces: {isVisible && <ExpensivePanel />}
<Activity mode={isVisible ? 'visible' : 'hidden'}>
  <ExpensivePanel />
</Activity>
```

When `mode="hidden"`: effects unmount, updates defer (off-screen priority). When `mode="visible"`: effects mount, updates process normally. State is preserved across both modes.

Use for: tab panels that should not remount, routes that need to preserve scroll position, any panel where remounting is expensive.

### `useEffectEvent` — Stable Event Callbacks in Effects

For callbacks inside `useEffect` that need to read latest props/state without being re-listed as dependencies:

```tsx
import { useEffectEvent } from 'react'

function ChatRoom({ roomId, theme }) {
  // onConnected always reads latest `theme`, but is not a dep of the effect
  const onConnected = useEffectEvent(() => {
    showNotification('Connected!', theme)
  })

  useEffect(() => {
    const conn = createConnection(roomId)
    conn.on('connected', onConnected)
    return () => conn.disconnect()
  }, [roomId]) // no `theme` needed here — useEffectEvent handles it
}
```

**Rule**: use `useEffectEvent` instead of suppressing `react-hooks/exhaustive-deps` for event-like callbacks. Requires `eslint-plugin-react-hooks` v6 (flat config).

---

