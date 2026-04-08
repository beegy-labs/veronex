# Tailwind CSS v4 — Research

> **Last Researched**: 2026-03-02 | **Source**: Official docs + web search + implementation
> **Status**: Verified — 4-layer token architecture in production (`web/app/tokens.css`)
> **Companion**: `research/frontend/tailwind-2026.md` — breaking changes + new utilities

---

## Core Change: CSS-First Configuration

No `tailwind.config.ts` in v4. Everything moves to your CSS entry file:

```css
/* globals.css */
@import "tailwindcss";

@theme {
  --color-primary: #0f3325;
  --radius-md: 0.375rem;
  --font-sans: "Inter", sans-serif;
}
```

Every `@theme` variable generates:
1. A Tailwind utility class (e.g., `text-primary`, `rounded-md`, `font-sans`)
2. A live CSS custom property accessible via `var(--color-primary)`

**PostCSS setup** (`postcss.config.mjs`):
```js
export default { plugins: { '@tailwindcss/postcss': {} } }
```

---

## 4-Layer Token Architecture (this codebase SSOT)

**File:** `web/app/tokens.css`

```css
/* Layer 1: @property — typed, animated, transition-safe CSS vars */
@property --theme-primary {
  syntax: '<color>';
  inherits: false;
  initial-value: transparent;
}

/* Layer 2: --palette-* — raw values, no semantic meaning */
:root {
  --palette-green-900: #0d2518;
  --palette-green-700: #16402e;
}

/* Layer 3: --theme-* — semantic aliases, light/dark aware */
:root {
  --theme-primary: #0f3325;          /* Deep Ivy (12.71:1 AAA) */
  --theme-bg-base: #f2f4f2;          /* Platinum Pearl */
  --theme-bg-card: #ffffff;
  --theme-border: #e2e8e4;
  --theme-text-primary: #0f3325;
}
.dark {
  --theme-primary: #10b981;          /* Bio-Emerald (7.73:1 AAA) */
  --theme-bg-base: #080a09;
  --theme-bg-card: #111412;
  --theme-border: #1f2b23;
  --theme-text-primary: #e8ede9;
}

/* Layer 4: @theme inline — tells Tailwind to generate utility classes */
@theme inline {
  --color-primary: var(--theme-primary);
  --color-background: var(--theme-bg-base);
  --color-card: var(--theme-bg-card);
  --color-border: var(--theme-border);
  --color-text: var(--theme-text-primary);
}
```

**Rule:** Never hardcode hex values in JSX or component CSS. Always reference `--theme-*` vars or generated utilities.

---

## Custom Utilities with `@utility`

Use `@utility` (not `@layer utilities`) for custom utilities that need responsive + variant support:

```css
/* CORRECT — supports hover:, dark:, sm: prefixes */
@utility scrollbar-hide {
  scrollbar-width: none;
  &::-webkit-scrollbar { display: none; }
}

/* WRONG — avoid @layer utilities for new utilities */
@layer utilities {
  .scrollbar-hide { ... }  /* does NOT support variants */
}
```

---

## Container Queries (2026 pattern)

Use container queries for reusable components that adapt to their parent's size (not the viewport):

```css
/* Define a containment context */
.card-wrapper {
  container-type: inline-size;
  container-name: card;
}

/* Component adapts to container */
@container card (min-width: 400px) {
  .card-content { flex-direction: row; }
}
```

In Tailwind v4, container queries are built-in:
```tsx
<div className="@container">
  <div className="flex-col @md:flex-row">...</div>
</div>
```

**When to use:** Reusable dashboard cards, data table cells, sidebar items.
**When not:** Page-level layout (use `sm:`, `lg:` viewport breakpoints).

---

## Lightning CSS Engine

Tailwind v4 uses Lightning CSS (Rust-based) instead of PostCSS for transforms:
- **Full builds:** ~5x faster than v3
- **Incremental builds:** ~100x faster (sub-millisecond HMR)
- **Automatic vendor prefixing** — no `autoprefixer` needed
- **Native CSS nesting** — `&:hover { }` works without plugin

Remove `autoprefixer` from PostCSS config if migrating from v3.

---

## Anti-Patterns

| Anti-pattern | Correct approach |
|---|---|
| `className="text-[#0f3325]"` hardcoded hex | `className="text-primary"` (generated from `@theme`) |
| `className="bg-white dark:bg-gray-950"` | `className="bg-card"` (semantic token) |
| `@apply flex items-center gap-2` everywhere | Write utility classes in JSX; `@apply` only for shared component abstractions |
| `tailwind.config.ts` still present | Delete it — v4 CSS file is the SSOT |
| `@layer utilities {}` for custom classes | Use `@utility` instead (supports variants) |

---

## Chart Tooltip Colors (this codebase)

Recharts tooltips must use CSS vars for theme correctness:

```tsx
// CORRECT — CSS vars work in Recharts labelStyle/itemStyle
<Tooltip
  contentStyle={{ background: 'var(--theme-bg-card)', border: '1px solid var(--theme-border)' }}
  labelStyle={{ color: 'var(--theme-text-primary)' }}
  itemStyle={{ color: 'var(--theme-text-primary)' }}
/>
```

**SSOT:** `docs/llm/frontend/charts.md` (chart-theme.ts constants)

---

## Sources

- Tailwind CSS v4 official docs: https://tailwindcss.com/docs
- Tailwind v4 release blog: https://tailwindcss.com/blog/tailwindcss-v4
- Verified: `web/app/tokens.css`, `web/app/globals.css`, `web/postcss.config.mjs`
