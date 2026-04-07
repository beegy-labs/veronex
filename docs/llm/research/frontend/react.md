# React — 2026 Research

> **Last Researched**: 2026-04-07 | **Source**: Web search + verified in production
> **Status**: Verified — patterns used throughout `web/` codebase

---

## React Compiler (v1.0, October 2025)

| Item | Detail |
|------|--------|
| Status | Stable; opt-in for existing codebases via Babel/SWC plugin |
| Rule | Remove `useMemo`/`useCallback`/`React.memo` from 95% of components |
| Escape hatch | `'use no memo'` at function level |
| Migration | Run `eslint-plugin-react-compiler` first (surfaces Rules of React violations) |

Keep manual memoization only when:
- Third-party lib requires reference equality compiler can't satisfy (react-dnd, animation libs)
- Library uses interior mutability compiler can't see through (react-hook-form `watch`)
- Profiler-measured hot path compiler's heuristics miss

> Sources: [React Compiler docs](https://react.dev/learn/react-compiler) | [BSWEN — useMemo decision 2026](https://docs.bswen.com/blog/2026-02-28-react-forget-usememo-usecallback/)

---

## React 19 Hooks — Decision Matrix

| Hook | Use when |
|------|----------|
| `use(promise)` | Unwrap a Promise passed as prop from Server Component; conditional context reads |
| `useActionState(fn, init)` | Any form/mutation needing pending + error + result state — replaces `useState` + `useEffect` patterns |
| `useOptimistic(state, fn)` | UX needs instant feedback before server confirms (toggles, likes, sends) |
| `useFormStatus()` | Child component needs parent form's pending state — eliminates prop drilling |

### `useOptimistic` pattern

```tsx
const [optimistic, addOptimistic] = useOptimistic(
  realState,
  (current, delta) => ({ ...current, ...delta })
)
// Inside action: addOptimistic(newValue); then await serverCall()
```

React automatically reverts to `realState` on failure or when action completes. Pairs naturally with `useActionState` and `useMutation`.

### `forwardRef` removal (React 19)

Refs are now plain props — no wrapper needed:

```tsx
// React 19 — ref as plain prop
function Input({ ref, ...props }) {
  return <input ref={ref} {...props} />
}

// React 18 — required forwardRef wrapper (obsolete)
const Input = React.forwardRef((props, ref) => <input ref={ref} {...props} />)
```

> Sources: [React 19 blog](https://react.dev/blog/2024/12/05/react-19) | [Deep Dive React 19 hooks](https://medium.com/@rohitkuwar/deep-dive-into-react-19s-latest-hooks)

---

## useReducer vs useState

| | `useReducer` | `useState` |
|--|-------------|-----------|
| **Multiple sub-values** | Yes — Single dispatch, atomic | Caution — Multiple setters, race risk |
| **Stable dispatch ref** | Yes — Never changes | No — Setter is stable but closure risk |
| **Complex transitions** | Yes — Action-based, testable | Caution — Logic scattered in event handlers |
| **Simple scalar** | Caution — Verbose | Yes — Simpler |

**Rule**: Use `useReducer` when state has multiple fields, or when multiple actions mutate it.

```tsx
// useReducer for particle/list state
type Action = { type: 'SPAWN'; items: Item[] } | { type: 'EXPIRE'; id: string }

function reducer(state: Item[], action: Action): Item[] {
  switch (action.type) {
    case 'SPAWN':  return [...state, ...action.items].slice(-MAX)
    case 'EXPIRE': return state.filter(i => i.id !== action.id)
  }
}

const [items, dispatch] = useReducer(reducer, [])
```

---

## ResizeObserver — Responsive Logical Space

Preferred over `window.resize` + debounce.

```tsx
const containerRef = useRef<HTMLDivElement>(null)
const [scale, setScale] = useReducer((_: number, v: number) => v, 1)

useEffect(() => {
  if (!containerRef.current) return
  const obs = new ResizeObserver(([entry]) => {
    setScale(entry.contentRect.width / LOGICAL_WIDTH)
  })
  obs.observe(containerRef.current)
  return () => obs.disconnect()   // cleanup on unmount
}, [])
```

- `useReducer` for scale state: single-value reducer avoids extra function wrapper
- `obs.disconnect()` in cleanup prevents memory leak
- No deps array needed — ref is stable

---

## onAnimationEnd — Cleanup without Leaks

```tsx
// onAnimationEnd: fires once, exactly when CSS animation ends
<div
  className="animated-particle"
  onAnimationEnd={() => dispatch({ type: 'EXPIRE', id })}
/>

// AVOID: setTimeout: timing guesswork, leaks on unmount
useEffect(() => {
  const t = setTimeout(() => remove(id), DURATION_MS)
  return () => clearTimeout(t)   // must manually manage
}, [id])
```

**Note**: If the element may be removed before animation ends, prefer `onAnimationEnd` —
React's synthetic event system handles this safely.

---

## useMemo — Dependency Array Rules

> **2026 note:** React Compiler (v1.0) handles most `useMemo` automatically. Only write manual `useMemo` for the exceptions listed in the React Compiler section above.

```tsx
// AVOID: WRONG — Date.now() in dep array → new value every render → no memoization
const cutoff = Date.now() - WINDOW_MS  // declared outside useMemo
const filtered = useMemo(() => items.filter(i => i.ts > cutoff), [items, cutoff])

// CORRECT — Date.now() computed inside useMemo body
const filtered = useMemo(() => {
  const cutoff = Date.now() - WINDOW_MS   // local variable, not a dependency
  return items.filter(i => i.ts > cutoff)
}, [items])
```

---

## useRef for "seen" Sets (deduplication across renders)

When tracking "has this item been processed" across renders without re-rendering:

```tsx
const seenIds = useRef<Set<string>>(new Set())

// On new data:
const newItems = data.filter(d => !seenIds.current.has(d.id))
newItems.forEach(d => seenIds.current.add(d.id))
```

`useRef` persists across renders without causing re-renders. Do NOT put mutable sets in `useState`.

---

## First-Load Snapshot Pattern

Prevents animating pre-existing data on mount (only animate truly new items):

```tsx
const initialized = useRef(false)

useEffect(() => {
  if (!data) return

  if (!initialized.current) {
    // Snapshot existing — mark as seen without animating
    data.forEach(d => seenIds.current.add(d.id))
    initialized.current = true
    return
  }

  // Subsequent polls: only new items
  const newItems = data.filter(d => !seenIds.current.has(d.id))
  // ... animate newItems
}, [data])
```

---

## Sources

- React docs: [useReducer](https://react.dev/reference/react/useReducer)
- MDN: [ResizeObserver](https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver)
- Verified: `web/hooks/use-inference-stream.ts`, `web/app/overview/components/provider-flow-panel.tsx`
