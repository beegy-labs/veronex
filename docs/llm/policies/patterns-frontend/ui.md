# Frontend Patterns — UI, Icons & Accessibility

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## SVG Pattern IDs — `useId()` for DOM Uniqueness

SVG `<pattern id="...">` elements use global DOM IDs. If a component can render multiple instances, use `React.useId()` to generate unique IDs. Strip `:` from the result — React IDs like `:r1:` are not valid XML NCNames.

```tsx
import { useId } from 'react'

const rawId = useId()
const safeId = rawId.replace(/:/g, '') // React IDs contain ':' which is invalid in SVG NCNames
const patternId = `my-pattern-${safeId}`

// In SVG:
<pattern id={patternId} .../>
<rect fill={`url(#${patternId})`} />
```

---

## Accessibility — WCAG 2.1 AA (Admin Dashboard Scope)

| Criterion | Check |
|-----------|-------|
| 1.4.1 Use of Color | Status conveyed by color MUST also have icon or text |
| 1.4.3 Contrast | Min 4.5:1 for normal text — design tokens already exceed AA; flag hardcoded low-contrast |
| 2.1.1 Keyboard | All interactive elements reachable by Tab; dialogs trap focus |
| 2.4.7 Focus Visible | All focusable elements have `focus-visible:` ring — use `--theme-focus-ring` token |
| 4.1.2 Name/Role/Value | Icon-only buttons → `aria-label`; form inputs → `<Label>` or `aria-label` |
| Loading states | Spinner/skeleton → `aria-label="Loading"` or `aria-busy` |

Not applicable: 1.2.x (no media), 1.4.4 resize (browser-native), 2.4.5 multiple ways (single-page admin).

---

## lucide-react v1 Patterns

### `LucideProvider` — Global Icon Defaults

Set default `size`, `strokeWidth`, and `color` for all icons in a subtree without prop drilling:

```tsx
import { LucideProvider } from 'lucide-react'

// In a layout or section that uses many icons
<LucideProvider size={16} strokeWidth={1.5}>
  <Nav />
  <Sidebar />
</LucideProvider>
```

Individual icon props override the provider. Use instead of repeating `size={16}` on every icon.

### `aria-hidden` Default (v1 behavior change)

All icons now render with `aria-hidden="true"` by default. This is correct for decorative icons. For icon-only buttons or standalone status icons that convey meaning, explicitly override:

```tsx
// Decorative — no change needed (aria-hidden="true" is the default)
<Trash2 className="h-4 w-4" />

// Semantic — must override (icon conveys meaning without visible text)
<AlertCircle aria-hidden={false} aria-label={t('status.error')} className="h-4 w-4" />
```

Rule: any icon used as the *only* indicator of meaning (no adjacent text) needs `aria-hidden={false}` + `aria-label`.

### CSS Class Name Drift

When icons are renamed, lucide keeps the old import name as an alias (TypeScript keeps compiling) but the rendered SVG emits the **canonical new class name**. Example:

```tsx
import { Home } from 'lucide-react'
// Import compiles fine, but SVG emits class="lucide lucide-house" (not lucide-home)
```

**Rule**: do not use `lucide-*` CSS selectors — they are fragile across renames. Style icons via `className` prop only.

### `createLucideIcon` — Custom Icons

For custom SVG icons that should match Lucide's conventions (size, strokeWidth, color props):

```tsx
import { createLucideIcon } from 'lucide-react'

const HexIcon = createLucideIcon('hex-icon', [
  ['polygon', { points: '12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5' }],
])

// Usage — identical to any Lucide icon
<HexIcon size={20} strokeWidth={1.5} />
```

Prefer this over hand-rolling SVG components when you need size/color/strokeWidth props to work correctly.

### `DynamicIcon` — CMS-Driven Icons (bundle warning)

```tsx
import { DynamicIcon } from 'lucide-react/dynamic'
<DynamicIcon name="camera" size={24} />
```

**Warning**: this separate entry point imports all icons into the bundle and bypasses tree-shaking. Only use when icon names are genuinely unknown at build time (CMS content, user-configurable). Never use for static UI.

---

