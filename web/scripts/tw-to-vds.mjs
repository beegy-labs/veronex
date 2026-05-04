#!/usr/bin/env node
/**
 * Tailwind → verodesign codemod for veronex/web.
 *
 * Strategy: preserve syntactic context.
 *   1. Pre-scan the file to compute byte ranges that are "code" — i.e. NOT
 *      inside `// ...` line comments, `/* ... *‍/` block comments, or template
 *      literal text. Only string literals embedded in code regions are
 *      transformed. This avoids the trap where an apostrophe in English prose
 *      ("page's") inside a comment offsets all subsequent regex pairings.
 *   2. Within "code" regions, find single-line single- or double-quoted
 *      strings. Treat each as a candidate className string.
 *   3. If the literal looks className-ish, tokenise on whitespace, transform
 *      each token, rejoin.
 *
 * Token transform:
 *   - Preserve `vds-*` (already migrated).
 *   - State / responsive prefixes (hover, focus, focus-visible, active,
 *     disabled, group-hover, placeholder, sm, md, lg, xl, 2xl): rewrite as
 *     `vds-{prefix}:{rest}` — the suffix stays bare per verodesign convention.
 *   - `dark:` prefix: drop it (verodesign uses light-dark()).
 *   - Apply semantic renames (foreground/background/popover/etc → vds slots).
 *   - For recognised Tailwind utilities, prefix `vds-`.
 */

import { readFile, writeFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import { collect } from './_globby.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const SEMANTIC = new Map([
  ['bg-background', 'bg-page'],
  ['bg-popover', 'bg-card'],
  ['bg-input', 'bg-card'],
  ['bg-secondary', 'bg-elevated'],
  ['bg-accent', 'bg-hover'],
  ['bg-foreground', 'bg-primary'],
  ['bg-status-success', 'bg-success'],
  ['bg-status-error', 'bg-error'],
  ['bg-status-warning', 'bg-warning'],
  ['bg-status-info', 'bg-info'],
  ['bg-status-cancelled', 'bg-cancelled'],
  ['text-foreground', 'text-primary'],
  ['text-background', 'text-primary-fg'],
  ['text-card-foreground', 'text-primary'],
  ['text-popover-foreground', 'text-primary'],
  ['text-primary-foreground', 'text-primary-fg'],
  ['text-destructive-foreground', 'text-destructive-fg'],
  ['text-secondary-foreground', 'text-primary'],
  ['text-accent-foreground', 'text-primary'],
  ['text-muted-foreground', 'text-dim'],
  ['text-status-success-fg', 'text-success'],
  ['text-status-error-fg', 'text-error'],
  ['text-status-warning-fg', 'text-warning'],
  ['text-status-info-fg', 'text-info'],
  ['border-border', 'border-subtle'],
  ['border-input', 'border-default'],
  ['border-ring', 'border-focus'],
  ['ring-ring', 'ring-focus'],
  ['ring-offset-background', 'ring-offset-page'],
  ['font-normal', 'font-400'],
  ['font-medium', 'font-500'],
  ['font-semibold', 'font-600'],
  ['font-bold', 'font-700'],
  ['shadow-sm', 'shadow-1'],
  ['shadow-md', 'shadow-2'],
  ['shadow-lg', 'shadow-3'],
  ['shadow-xl', 'shadow-4'],
  ['shadow-2xl', 'shadow-5'],
  ['shadow-inner', 'shadow-inner'],
  ['tracking-tighter', 'tracking-tight'],
  ['duration-75', 'duration-fast'],
  ['duration-100', 'duration-fast'],
  ['duration-150', 'duration-medium'],
  ['duration-200', 'duration-medium'],
  ['duration-300', 'duration-slow'],
  ['duration-500', 'duration-slow'],
  ['duration-700', 'duration-slower'],
  ['ease-in', 'ease-ease-in'],
  ['ease-out', 'ease-ease-out'],
  ['ease-in-out', 'ease-ease-in-out'],
])

const PASSTHROUGH = new Set([
  'animate-in', 'animate-out',
  'fade-in-0', 'fade-out-0',
  'zoom-in-95', 'zoom-out-95',
  'slide-in-from-top-2', 'slide-in-from-bottom-2',
  'slide-in-from-left-2', 'slide-in-from-right-2',
  'slide-in-from-left-1/2', 'slide-in-from-top-[48%]',
  'slide-out-to-left-1/2', 'slide-out-to-top-[48%]',
  'skip-link', 'vds-sr-only', 'bee-particle', 'sr-only', 'not-sr-only',
])

const RECOGNISED = [
  /^(p|m)[xytrbl]?-/,
  /^(w|h|gap|space-x|space-y|gap-x|gap-y|min-w|max-w|min-h|max-h|size)-/,
  /^(bg|text|border|fill|stroke|outline|ring|divide|placeholder)-/,
  /^(rounded|shadow|opacity|cursor|overflow|truncate|italic|uppercase|lowercase|capitalize|underline|line-through)/,
  /^(font|leading|tracking|whitespace|break)-/,
  /^(flex|grid|inline-flex|inline-grid|inline-block|inline|block|hidden|contents|table)$/,
  /^(items|justify|self|content|place)-/,
  /^flex-(1|auto|none|row|col|wrap|nowrap|grow|shrink)$/,
  /^(grow|shrink)$/,
  /^(grid-cols|grid-rows|col-span|row-span|col-start|col-end|row-start|row-end)-/,
  /^(relative|absolute|fixed|sticky|static)$/,
  /^(inset|top|right|bottom|left|z)-/,
  /^transition(-(?:none|all|colors|opacity|shadow|transform))?$/,
  /^(duration|ease|delay|animate)/,
  /^(transform|translate|rotate|scale|skew|origin)-/,
  /^(blur|backdrop)/,
  /^(pointer-events|select|appearance)-/,
  /^border-(t|r|b|l|x|y)/,
  /^(visible|invisible)$/,
  /^(border|shadow|ring)$/,
]

const PREFIX_RE = /^(hover|focus|focus-visible|active|disabled|group-hover|placeholder|sm|md|lg|xl|2xl):(.+)/

function applySemantic(rest) {
  const slash = rest.indexOf('/')
  if (slash >= 0) {
    const head = rest.slice(0, slash)
    const tail = rest.slice(slash)
    const repl = SEMANTIC.get(head)
    return repl ? repl + tail : rest
  }
  return SEMANTIC.get(rest) ?? rest
}

function isRecognisedTailwind(rest) {
  const head = rest.replace(/\/[\w.\-]+$/, '')
  return RECOGNISED.some((p) => p.test(head))
}

function transformToken(token) {
  if (!token) return token
  if (token.startsWith('vds-')) return token
  if (PASSTHROUGH.has(token)) return token
  if (token.startsWith('dark:')) return ''

  const m = token.match(PREFIX_RE)
  if (m) {
    const prefix = m[1]
    let rest = m[2]
    rest = applySemantic(rest)
    return `vds-${prefix}:${rest}`
  }

  const renamed = applySemantic(token)
  if (isRecognisedTailwind(renamed)) {
    return `vds-${renamed}`
  }
  return renamed
}

function transformClassString(s) {
  return s
    .split(/(\s+)/)
    .map((tok) => (/\S/.test(tok) ? transformToken(tok) : tok))
    .join('')
    .replace(/[ \t]+/g, ' ')
    .trim()
}

function looksLikeClassNames(s) {
  if (!s || s.length > 4000) return false
  return /(?:^|\s)(?:vds-|hover:|focus:|focus-visible:|disabled:|active:|placeholder:|group-hover:|sm:|md:|lg:|xl:|2xl:|dark:|bg-|text-|border|fill-|stroke-|outline-|ring|divide-|placeholder-|flex|inline-flex|grid|inline-grid|inline-block|inline|hidden|block|contents|table|absolute|relative|fixed|sticky|static|truncate|italic|uppercase|lowercase|capitalize|underline|line-through|grow|shrink|p[xytrbl]?-|m[xytrbl]?-|space-[xy]-|gap(-[xy])?-|w-|h-|min-w-|max-w-|min-h-|max-h-|size-|rounded|shadow|opacity-|cursor-|overflow-|font-|leading-|tracking-|whitespace-|break-|items-|justify-|self-|content-|place-|inset-|top-|right-|bottom-|left-|z-|grid-cols-|grid-rows-|col-span-|row-span-|col-start-|col-end-|row-start-|row-end-|transition|duration-|ease-|delay-|animate-|transform|translate-|rotate-|scale-|skew-|origin-|blur-|backdrop-|pointer-events-|select-|appearance-|visible|invisible)/.test(
    s,
  )
}

/**
 * Compute "code" regions of the file — byte ranges OUTSIDE comments and
 * outside the textual body of multi-line template literals. Within those
 * regions, single- and double-quoted string literals are safe to scan.
 *
 * Uses a tiny state machine. Tracks: code | sline-string | dline-string |
 * line-comment | block-comment | template-literal.
 *
 * Returns a list of single-line string literal ranges [{ start, end, quote }]
 * inside code (not inside comments / templates).
 */
function findClassCandidates(src) {
  const ranges = []
  const len = src.length
  let i = 0
  let lineStart = 0
  // 0=code,1=//,2=/* */, 3=template, 4='-string, 5=" -string
  const STATE_CODE = 0
  const STATE_LINECOMMENT = 1
  const STATE_BLOCKCOMMENT = 2
  const STATE_TEMPLATE = 3
  const STATE_SQ = 4
  const STATE_DQ = 5
  let state = STATE_CODE
  let stringStart = -1

  while (i < len) {
    const c = src[i]
    const next = src[i + 1]
    if (c === '\n') lineStart = i + 1

    switch (state) {
      case STATE_CODE: {
        if (c === '/' && next === '/') { state = STATE_LINECOMMENT; i += 2; continue }
        if (c === '/' && next === '*') { state = STATE_BLOCKCOMMENT; i += 2; continue }
        if (c === '`') { state = STATE_TEMPLATE; i++; continue }
        if (c === "'") { state = STATE_SQ; stringStart = i + 1; i++; continue }
        if (c === '"') { state = STATE_DQ; stringStart = i + 1; i++; continue }
        i++; continue
      }
      case STATE_LINECOMMENT: {
        if (c === '\n') { state = STATE_CODE }
        i++; continue
      }
      case STATE_BLOCKCOMMENT: {
        if (c === '*' && next === '/') { state = STATE_CODE; i += 2; continue }
        i++; continue
      }
      case STATE_TEMPLATE: {
        if (c === '\\') { i += 2; continue }
        if (c === '`') { state = STATE_CODE; i++; continue }
        // Skip ${...} interpolation payload — re-enter code mode for that span,
        // but for simplicity we just treat the entire template body as opaque.
        // No className extraction inside templates (consumers should write
        // className strings as plain literals or pass them via cn()).
        i++; continue
      }
      case STATE_SQ:
      case STATE_DQ: {
        const closer = state === STATE_SQ ? "'" : '"'
        if (c === '\\') { i += 2; continue }
        if (c === '\n') {
          // Multi-line single/double quoted string — abort and treat content
          // as code (recover on the next quote of the same kind).
          state = STATE_CODE
          stringStart = -1
          i++; continue
        }
        if (c === closer) {
          // Capture the literal range
          ranges.push({ start: stringStart, end: i, quote: closer })
          state = STATE_CODE
          stringStart = -1
          i++; continue
        }
        i++; continue
      }
    }
  }
  void lineStart
  return ranges
}

function processFile(content) {
  const ranges = findClassCandidates(content)
  if (ranges.length === 0) return { out: content, count: 0 }

  let count = 0
  // Build the new content by walking ranges in order.
  let out = ''
  let cursor = 0
  for (const r of ranges) {
    const body = content.slice(r.start, r.end)
    out += content.slice(cursor, r.start)
    if (looksLikeClassNames(body)) {
      const updated = transformClassString(body)
      if (updated !== body) {
        out += updated
        count++
      } else {
        out += body
      }
    } else {
      out += body
    }
    cursor = r.end
  }
  out += content.slice(cursor)
  return { out, count }
}

async function main() {
  const files = await collect(
    ['app', 'components', 'hooks', 'lib'].map((d) => path.join(ROOT, d)),
    ['.tsx', '.ts'],
  )

  let totalFiles = 0
  let totalRewrites = 0
  for (const file of files) {
    const original = await readFile(file, 'utf8')
    const { out, count } = processFile(original)
    if (count > 0 && out !== original) {
      await writeFile(file, out, 'utf8')
      totalFiles++
      totalRewrites += count
      const rel = path.relative(ROOT, file)
      console.log(`  ${rel}  (${count} literals updated)`)
    }
  }
  console.log(`\n✓ ${totalFiles} file(s) updated, ${totalRewrites} string literals rewritten.`)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
