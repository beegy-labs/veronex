/**
 * vds-merge — last-wins deduplication for verodesign utility classes.
 *
 * Replaces tailwind-merge (forbidden after migration). When a component's
 * baseline classes are followed by user overrides, conflicting utilities in
 * the same property group should let the LAST occurrence win — otherwise
 * CSS source order in full.css decides arbitrarily and overrides silently
 * fail (e.g. user `vds-h-8` loses to default `vds-h-9` because `.vds-h-9`
 * is emitted later in the stylesheet).
 *
 * We map each utility class to a "conflict key" representing the CSS
 * property family it touches. Within a conflict key, only the last token
 * is kept; tokens whose key is unknown are kept verbatim in original order.
 *
 * State prefixes (`vds-hover:`, `vds-md:`, `vds-focus:`, …) are scoped: a
 * `vds-hover:bg-hover` does not conflict with a base-state `vds-bg-card`.
 */

/**
 * Build a conflict key for a single class token.
 *
 *   vds-h-8                   → "h"
 *   vds-h-[100px]             → "h"
 *   vds-px-4                  → "px"
 *   vds-text-sm               → "text-size"
 *   vds-text-primary          → "text-color"
 *   vds-bg-hover              → "bg"
 *   vds-rounded-full          → "rounded"
 *   vds-hover:bg-hover        → "hover:bg"
 *
 * Returns null when the token isn't in any known conflict group, meaning
 * the merger keeps it verbatim. Erring on the side of "no conflict" is
 * safer than over-grouping and discarding legitimate classes.
 */
const SIZE_SUFFIXES = new Set([
  'xs', 'sm', 'base', 'md', 'lg', 'xl',
  '2xl', '3xl', '4xl', '5xl', '6xl', '7xl', '8xl', '9xl',
])

/* eslint-disable */
function conflictKeyForCore(core: string): string | null {
  // First-segment prefix (everything up to the first `-`).
  const dashIdx = core.indexOf('-')
  if (dashIdx <= 0) return null
  const head = core.slice(0, dashIdx)
  const rest = core.slice(dashIdx + 1)

  // text-* — every variant ultimately sets `color` OR `font-size`. Group by
  // CSS property:
  //   text-{xs|sm|base|lg|xl|2xl|...}  → font-size
  //   text-[…]                          → arbitrary value, treat as size
  //   text-{primary|primary-fg|dim|…}   → color
  if (head === 'text') {
    const seg = rest.split('/')[0]
    const firstSeg = seg.split('-')[0]
    if (SIZE_SUFFIXES.has(seg)) return 'text-size'
    if (SIZE_SUFFIXES.has(firstSeg) && /^\d/.test(seg.slice(firstSeg.length))) return 'text-size'
    if (/^\[/.test(seg)) return 'text-size'
    return 'text-color'
  }

  // font-* — weight (numeric) vs family (mono|sans|serif) vs other.
  if (head === 'font') {
    if (/^\d+$/.test(rest)) return 'font-weight'
    if (rest === 'mono' || rest === 'sans' || rest === 'serif') return 'font-family'
    return 'font'
  }

  // border / divide / ring / outline — ambiguous between width and color.
  //   border-{0|1|2|4|8}        → border-width
  //   border-{dashed|solid|…}   → border-style
  //   border-{subtle|default|primary|info|…} → border-color
  // Without disambiguation, `vds-border-1` (width) and `vds-border-subtle`
  // (color) collapse into one group and one of them gets dropped, which is
  // why Card/Input borders rendered as 0 px.
  if (head === 'border' || head === 'divide' || head === 'ring' || head === 'outline') {
    const seg = rest.split('/')[0]
    if (/^\d+$/.test(seg)) return `${head}-width`
    if (seg === 'dashed' || seg === 'solid' || seg === 'dotted' ||
        seg === 'double' || seg === 'none' || seg === 'hidden') {
      return `${head}-style`
    }
    // Side variants: border-t-2, border-l-1 → side-keyed width
    const sideMatch = rest.match(/^([trblxy])-(\d+)$/)
    if (sideMatch) return `${head}-${sideMatch[1]}-width`
    return `${head}-color`
  }

  // Multi-segment prefixes that semantically share a property family with
  // their first segment (e.g. min-w-* and max-w-* both set width-related
  // CSS, but they DON'T conflict with each other — keep them as separate
  // keys based on the leading two segments).
  const TWO_SEG_PREFIXES = new Set([
    'min-w', 'max-w', 'min-h', 'max-h',
    'gap-x', 'gap-y',
    'space-x', 'space-y',
    'border-t', 'border-r', 'border-b', 'border-l', 'border-x', 'border-y',
    'rounded-t', 'rounded-r', 'rounded-b', 'rounded-l',
    'rounded-tl', 'rounded-tr', 'rounded-br', 'rounded-bl',
    'inset-x', 'inset-y',
    'col-span', 'row-span', 'col-start', 'col-end', 'row-start', 'row-end',
    'grid-cols', 'grid-rows',
  ])
  const dash2 = rest.indexOf('-')
  if (dash2 > 0) {
    const twoSeg = head + '-' + rest.slice(0, dash2)
    if (TWO_SEG_PREFIXES.has(twoSeg)) return twoSeg
  }
  if (TWO_SEG_PREFIXES.has(core)) return core   // no value, e.g. would not happen but be safe

  // Single-segment prefix as the conflict key (h, w, p, px, py, pt, …,
  // m, mx, …, bg, border, rounded, shadow, opacity, leading, tracking,
  // z, top, right, bottom, left, inset, ring, outline).
  return head
}
/* eslint-enable */

const STATE_PREFIX_RE = /^(hover|focus|focus-visible|active|disabled|group-hover|placeholder|sm|md|lg|xl|2xl|dark):/

function tokenConflictKey(tok: string): string | null {
  // Singletons without a prefix-value structure (vds-flex, vds-block, vds-hidden,
  // vds-truncate, etc.) — these are display/visibility families. We deliberately
  // skip merging them; conflicts are rare and merging risks discarding a
  // legitimate combo (e.g. `vds-flex` + `vds-flex-col` are both flex-family but
  // both required).
  let core = tok
  let stateKey = ''

  if (core.startsWith('vds-')) {
    core = core.slice(4)
  } else {
    // Tokens not prefixed with `vds-` are passed through (we don't manage
    // foreign classes — they may belong to third-party widgets).
    return null
  }

  const sm = core.match(STATE_PREFIX_RE)
  if (sm) {
    stateKey = sm[0]   // includes the trailing colon
    core = core.slice(sm[0].length)
  }

  const k = conflictKeyForCore(core)
  return k ? stateKey + k : null
}

import { clsx, type ClassValue } from 'clsx'

export function cn(...inputs: ClassValue[]): string {
  const flat = clsx(inputs)
  if (!flat) return ''
  const tokens = flat.split(/\s+/).filter(Boolean)

  // Walk tokens once. Conflict-keyed tokens overwrite earlier same-key entries
  // in place; non-conflicting tokens are appended in encounter order.
  const slots: Array<{ key: string; tok: string }> = []
  const indexByKey = new Map<string, number>()

  for (const tok of tokens) {
    const k = tokenConflictKey(tok)
    if (k == null) {
      // Non-conflicting: keep verbatim, dedupe exact repeats.
      const dupKey = `__as_is__::${tok}`
      const i = indexByKey.get(dupKey)
      if (i == null) {
        indexByKey.set(dupKey, slots.length)
        slots.push({ key: dupKey, tok })
      }
      continue
    }
    const i = indexByKey.get(k)
    if (i == null) {
      indexByKey.set(k, slots.length)
      slots.push({ key: k, tok })
    } else {
      slots[i].tok = tok   // last wins, keep original position
    }
  }

  return slots.map((s) => s.tok).join(' ')
}
