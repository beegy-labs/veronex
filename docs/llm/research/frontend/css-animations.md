# CSS Animations — 2026 Research

> **Last Researched**: 2026-03-01 | **Source**: Web search + verified in production
> **Status**: ✅ Verified — used in `provider-flow-panel.tsx`, `server-dispatch-panel.tsx`

---

## Key Decision: CSS Motion Path over SVG SMIL

| | CSS Motion Path | SVG SMIL `animateMotion` |
|--|----------------|--------------------------|
| **GPU composited** | ✅ Yes — runs on compositor thread | ❌ No — main thread |
| **Dynamic spawn** | ✅ CSS `animation` starts on mount | ⚠️ Requires `begin="indefinite"` + `beginElement()` |
| **Cleanup** | ✅ `onAnimationEnd` event | ⚠️ `setTimeout` or SMIL `end` event |
| **Browser support** | ✅ All modern browsers (2023+) | ⚠️ Firefox dropped full SMIL support |
| **Tooling** | ✅ CSS devtools | ⚠️ Limited devtools support |

**Recommendation**: Always use CSS `offset-path: path(...)` for particle/motion animations.

---

## Pattern: Bee Particle System (used in this project)

### CSS

```css
/* globals.css */
@keyframes bee-fly {
  0%   { offset-distance: 0%;   opacity: 0; }
  6%   {                        opacity: 1; }
  88%  {                        opacity: 1; }
  100% { offset-distance: 100%; opacity: 0; }
}

.bee-particle {
  position: absolute;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  pointer-events: none;
  will-change: offset-distance, opacity;   /* GPU layer promotion */
  animation: bee-fly 1400ms linear forwards;
  offset-anchor: 50% 50%;
}
```

### Usage (React)

```tsx
<div
  className="bee-particle"
  style={{
    offsetPath: `path("M 200,54 C 200,112 105,112 105,170")`,
    backgroundColor: color,
    boxShadow: `0 0 6px 2px ${color}40`,
  }}
  onAnimationEnd={() => dispatch({ type: 'EXPIRE', id: bee.id })}
/>
```

---

## Pattern: Coordinate Alignment (CSS offset-path ↔ SVG viewBox)

**Problem**: `offset-path: path(...)` uses CSS pixel coordinates of the containing block,
not SVG viewBox units. If the SVG scales responsively, path coordinates drift.

**Solution**: Fixed logical space + CSS transform scale.

```tsx
const VIEW_W = 400
const VIEW_H = 240

// ResizeObserver measures container, derives scale factor
const obs = new ResizeObserver(([entry]) => {
  setScale(entry.contentRect.width / VIEW_W)
})

// SVG and bee overlay share the same 400×240 coordinate space
<svg viewBox={`0 0 ${VIEW_W} ${VIEW_H}`} className="absolute inset-0 w-full h-full" />

// Bee overlay: always 400×240, scaled to match SVG rendered size
<div
  style={{
    width: VIEW_W,
    height: VIEW_H,
    transform: `scale(${scale})`,
    transformOrigin: 'top left',
  }}
>
  {/* bee particles here — offset-path coords match SVG viewBox exactly */}
</div>
```

---

## Pattern: Particle State — useReducer over useState

```tsx
type Bee    = { id: string; pathD: string; color: string }
type Action = { type: 'SPAWN'; bees: Bee[] } | { type: 'EXPIRE'; id: string }

function beeReducer(state: Bee[], action: Action): Bee[] {
  switch (action.type) {
    case 'SPAWN':  return [...state, ...action.bees].slice(-MAX_BEES)  // cap at 15
    case 'EXPIRE': return state.filter(b => b.id !== action.id)
  }
}

const [bees, dispatch] = useReducer(beeReducer, [])

// Cleanup: onAnimationEnd (not setTimeout — avoids leak on unmount)
onAnimationEnd={() => dispatch({ type: 'EXPIRE', id: bee.id })}
```

**Why `useReducer`**: `dispatch` reference is stable across renders (unlike setState closure).
SPAWN and EXPIRE are atomic — no stale closure risk.

---

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| SVG `<animateMotion>` + `beginElement()` | Main thread, requires imperative ref per particle | CSS `offset-path` + `onAnimationEnd` |
| `setTimeout` for particle cleanup | Leaks on unmount, timing drift | `onAnimationEnd` event |
| `useState` for particle list | Closure over stale state in callbacks | `useReducer` |
| `Date.now()` in `useMemo` dep array | New value every render = no memoization | Compute `const cutoff = Date.now()` inside `useMemo` body |
| `will-change: transform` on offset-path | Wrong property — does not promote path animation | `will-change: offset-distance, opacity` |

---

## `will-change` Reference

```css
/* Particle moving along a path */
will-change: offset-distance, opacity;

/* Element sliding/scaling */
will-change: transform, opacity;

/* DO NOT overuse will-change — only apply to actively animating elements */
```

---

## Sources

- MDN: [CSS Motion Path](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_motion_path)
- Chrome Developers: GPU-composited animations (2024)
- Web search: "CSS offset-path vs SVG SMIL 2026 performance" → CSS Motion Path preferred
- Verified: `web/app/overview/components/provider-flow-panel.tsx`
- CSS: `web/app/globals.css` (`@keyframes bee-fly`, `.bee-particle`)
