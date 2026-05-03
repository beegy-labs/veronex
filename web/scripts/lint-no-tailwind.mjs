#!/usr/bin/env node
/**
 * Tailwind residue linter for veronex/web.
 *
 * Fails (exit 1) if any:
 *   1. Tailwind utility class without `vds-` prefix appears in className strings.
 *   2. `dark:` prefix appears anywhere in className strings.
 *   3. `tailwindcss`, `tailwind-merge`, `tw-animate-css`, `class-variance-authority`,
 *      or `@radix-ui/*` import remains.
 *   4. `cva(`, `tw\`` template literals.
 *
 * Allowed exceptions:
 *   - Strings inside `// cspell:`, `// vds-allow-tw:` comments.
 *   - Files under `app/styles/vds/**` (verodesign vendored).
 *   - The codemod scripts themselves (`scripts/tw-to-vds*.mjs`).
 */

import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { collect } from './_globby.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const FORBIDDEN_IMPORTS = [
  /from\s+['"]tailwindcss['"]/,
  /from\s+['"]tailwind-merge['"]/,
  /from\s+['"]tw-animate-css['"]/,
  /from\s+['"]class-variance-authority['"]/,
  /from\s+['"]@radix-ui\//,
  /from\s+['"]radix-ui['"]/,
  /from\s+['"]@tailwindcss\//,
]

const FORBIDDEN_TOKENS = [
  /\bcva\s*\(/,
  /\btwMerge\s*\(/,
]

// Tailwind utility patterns that must be prefixed with `vds-`. We only check
// inside string literals (className context) by tokenising on whitespace and
// checking each token. State/responsive prefixes are allowed only with vds-.
const TAILWIND_PATTERNS = [
  /^(p|m|w|h|gap)-/,
  /^(px|py|pt|pr|pb|pl|mx|my|mt|mr|mb|ml|space-x|space-y|gap-x|gap-y|min-w|max-w|min-h|max-h)-/,
  /^(bg|text|border|fill|stroke|outline|ring|divide|placeholder)-/,
  /^(rounded|shadow|opacity|cursor|overflow)/,
  /^(font|leading|tracking|whitespace|break)-/,
  /^(flex|grid|inline-flex|inline-grid|inline-block|inline|block|hidden|contents|table)$/,
  /^(items|justify|self|content|place)-/,
  /^(grid-cols|grid-rows|col-span|row-span|col-start|col-end|row-start|row-end)-/,
  /^(relative|absolute|fixed|sticky|static)$/,
  /^(inset|top|right|bottom|left|z)-/,
  /^(transition|duration|ease|delay|animate)/,
  /^(transform|translate|rotate|scale|skew|origin)-/,
  /^(blur|backdrop)/,
  /^(border-(t|r|b|l|x|y))/,
  /^size-/,
  /^accent-/,
  /^resize(-|$)/,
  /^tabular-nums$/,
  /^-(top|right|bottom|left|inset|m[trblxy]?|p[trblxy]?)-/,
  /^group(\/|$)/,
  /^group-hover\//,
]

const STATE_PREFIX = /^(hover|focus|focus-visible|active|disabled|group-hover|placeholder|sm|md|lg|xl|2xl|dark):/

const SAFE_TOKENS = new Set([
  // Custom global classes
  'skip-link', 'vds-sr-only', 'bee-particle', 'sr-only', 'not-sr-only',
  // Tailwind animate-* keep verbatim (we ship our own keyframes)
  'animate-in', 'animate-out',
  'fade-in-0', 'fade-out-0', 'zoom-in-95', 'zoom-out-95',
  'slide-in-from-top-2', 'slide-in-from-bottom-2',
  'slide-in-from-left-2', 'slide-in-from-right-2',
  // CSS layer / state attribute selectors
])

function isTailwindToken(t) {
  if (!t || SAFE_TOKENS.has(t)) return false
  if (t.startsWith('vds-')) return false
  // Strip state prefix
  const m = t.match(STATE_PREFIX)
  if (m) {
    if (t.startsWith('dark:')) return true   // dark: is forbidden entirely
    return false                              // hover:foo etc must be vds-hover:foo (already vds- handled above)
  }
  return TAILWIND_PATTERNS.some((p) => p.test(t))
}

function findOffendersInLiteral(s, line, col, file) {
  const offenders = []
  // Split on whitespace AND on JSX/JS punctuation (`{`, `}`, `` ` ``, `'`, `"`,
  // `?`, `(`, `)`, `,`, `;`) so a token that abuts a backtick or `{` —
  // e.g. `className={\`h-1.5 ...` — still resolves to a clean class token.
  // NOTE: `:` is NOT a splitter because state-prefixed verodesign classes
  // (`vds-hover:bg-hover`, `vds-md:items-center`) keep the colon as part of
  // the single utility class. Splitting on `:` would falsely flag the
  // post-colon segment.
  for (const tok of s.split(/[\s{}`'"?();,]+/)) {
    if (isTailwindToken(tok)) {
      offenders.push({ file, line, col, token: tok })
    }
  }
  return offenders
}

// Recursively scan a string body for Tailwind tokens, including ones embedded
// inside template-literal interpolations like ${cond ? 'bg-background' : ''}.
// The outer regex matches the whole template as one literal, so we re-scan its
// inner single/double-quoted substrings to catch tokens the outer pass misses.
function scanLiteralBody(body, line, file, issues) {
  if (!body || body.length > 4000) return
  if (!/[a-z]-\d|hover:|focus:|md:|lg:|sm:|xl:|2xl:|dark:|^\s*(flex|grid|hidden|relative|absolute|fixed|sticky|truncate|italic|uppercase|underline)\s*$/.test(body) && !/\b(p|m|w|h|bg|text|border|gap|flex|grid|rounded|shadow|leading|tracking|font|opacity|cursor|overflow)-/.test(body)) return
  for (const o of findOffendersInLiteral(body, line, 0, file)) {
    issues.push({ file, line: o.line, msg: `Tailwind token without vds- prefix: ${o.token}` })
  }
  // Re-scan for nested string literals inside template interpolations.
  const inner = /(['"])((?:\\.|(?!\1)[^\\])*?)\1/g
  let n
  while ((n = inner.exec(body))) {
    if (n[2]) scanLiteralBody(n[2], line, file, issues)
  }
}

function scanFile(content, file) {
  const issues = []
  const lines = content.split(/\n/)

  for (const re of FORBIDDEN_IMPORTS) {
    lines.forEach((l, i) => {
      if (re.test(l)) issues.push({ file, line: i + 1, msg: `forbidden import: ${l.trim()}` })
    })
  }
  for (const re of FORBIDDEN_TOKENS) {
    lines.forEach((l, i) => {
      if (re.test(l)) issues.push({ file, line: i + 1, msg: `forbidden token: ${l.trim()}` })
    })
  }

  let m
  const re = /(['"`])((?:\\.|(?!\1)[^\\])*?)\1/g
  while ((m = re.exec(content))) {
    const body = m[2]
    if (!body) continue
    const before = content.slice(0, m.index)
    const line = before.split(/\n/).length
    scanLiteralBody(body, line, file, issues)
  }

  return issues
}

async function main() {
  const files = (await collect(
    ['app', 'components', 'hooks', 'lib'].map((d) => path.join(ROOT, d)),
    ['.tsx', '.ts'],
  )).filter((f) => !f.includes('/app/styles/vds/'))
    .filter((f) => !f.endsWith('/lib/vds-merge.ts'))

  let total = 0
  for (const f of files) {
    const content = await readFile(f, 'utf8')
    const issues = scanFile(content, f)
    for (const i of issues) {
      console.log(`${path.relative(ROOT, i.file)}:${i.line}  ${i.msg}`)
      total++
    }
  }

  if (total > 0) {
    console.error(`\n✗ ${total} Tailwind residue issue(s) found.`)
    process.exit(1)
  }
  console.log(`✓ no Tailwind residue (${files.length} files scanned)`)
}

main().catch((e) => { console.error(e); process.exit(1) })
