# Tailwind CSS v4 — 2026 Updates

> **Last Researched**: 2026-04-07 | **Source**: Tailwind v4 docs + web search
> **Companion**: `research/frontend/tailwind.md` — core patterns

---

## Container Queries — Now in Core (no plugin)

Previously required `@tailwindcss/container-queries`. Zero-config in v4:

```html
<div class="@container">
  <div class="grid grid-cols-1 @sm:grid-cols-3 @lg:grid-cols-4">...</div>
</div>

<!-- Range query -->
<div class="flex @min-md:@max-xl:hidden">...</div>
<!-- Max-width -->
<div class="grid grid-cols-3 @max-md:grid-cols-1">...</div>
```

---

## Breaking Class Renames (v3 → v4)

| v3 | v4 |
|----|-----|
| `bg-gradient-to-r` | `bg-linear-to-r` |
| `bg-gradient-to-b` | `bg-linear-to-b` |
| `flex-shrink-0` | `shrink-0` |
| `flex-grow` | `grow` |

Auto-migrate: `npx @tailwindcss/upgrade`

---

## New Utilities

| Utility | Example |
|---------|---------|
| 3D transforms | `rotate-x-45`, `rotate-y-12`, `perspective-500` |
| Gradient angles | `bg-linear-45`, `bg-conic/[in_hsl_longer_hue]` |
| Enter/exit transitions | `starting:open:opacity-0` (CSS `@starting-style`, no JS) |
| Negative match | `not-hover:opacity-75`, `not-supports-hanging-punctuation:px-4` |
| Inset shadows | `inset-shadow-md`, `inset-ring-2` |
| Auto-resize textarea | `field-sizing-content` |
| Nth-child | `nth-odd:bg-muted`, `nth-3:font-bold` |
| Parent-less group | `in-hover:opacity-100` (like `group-hover` without `group`) |
| Dynamic values | `grid-cols-15`, `mt-17` (no arbitrary `[]` syntax needed) |

---

## Build Performance

| Metric | v3.4 | v4.0 |
|--------|------|------|
| Full build | 378ms | 100ms |
| Incremental (new CSS) | 44ms | 5ms |
| Incremental (no new CSS) | 35ms | 192µs |

---

## Browser Targets

Safari 16.4+, Chrome 111+, Firefox 128+. Uses `cascade layers`, `@property`, `color-mix()`.

---

## Sources

- [Tailwind v4 Complete Guide 2026](https://devtoolbox.dedyn.io/blog/tailwind-css-v4-complete-guide)
- [Tailwind v4 Tips — Nikolai Lehbrink](https://www.nikolailehbr.ink/blog/tailwindcss-v4-tips/)
