#!/usr/bin/env node
/**
 * Post-process the tw-to-vds codemod output to fix two specific issues:
 *
 *  1. Double-prefix in state/responsive variants. Verodesign expects
 *     `vds-hover:bg-card` (suffix bare). Source files that already had
 *     `'hover:vds-bg-card'` got rewritten to `'vds-hover:vds-bg-card'`.
 *     We strip the inner `vds-` from the suffix.
 *
 *  2. False-positive on string enum `'bottom-start'` — floating-ui
 *     placement strings were misinterpreted as Tailwind tokens and turned
 *     into `'vds-bottom-start'`. We restore the canonical floating-ui
 *     placement strings.
 */
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { collect } from './_globby.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PLACEMENT_RESTORES = [
  ['vds-bottom-start', 'bottom-start'],
  ['vds-bottom-end', 'bottom-end'],
  ['vds-top-start', 'top-start'],
  ['vds-top-end', 'top-end'],
  ['vds-left-start', 'left-start'],
  ['vds-left-end', 'left-end'],
  ['vds-right-start', 'right-start'],
  ['vds-right-end', 'right-end'],
]

function fixDoublePrefix(content) {
  // vds-{state}:vds-{rest}  →  vds-{state}:{rest}
  // States: hover focus focus-visible active disabled group-hover placeholder
  // Responsive: sm md lg xl 2xl
  return content.replace(
    /\bvds-(hover|focus|focus-visible|active|disabled|group-hover|placeholder|sm|md|lg|xl|2xl):vds-/g,
    'vds-$1:',
  )
}

function fixPlacements(content) {
  let out = content
  for (const [from, to] of PLACEMENT_RESTORES) {
    // Only fix when used as a quoted string literal (placement enum), not
    // accidentally inside another vds- token.
    out = out.replace(new RegExp(`(['"])${from}\\1`, 'g'), `$1${to}$1`)
  }
  return out
}

async function main() {
  const files = await collect(
    ['app', 'components', 'hooks', 'lib'].map((d) => path.join(ROOT, d)),
    ['.tsx', '.ts'],
  )
  let updated = 0
  for (const f of files) {
    const orig = await readFile(f, 'utf8')
    let next = orig
    next = fixDoublePrefix(next)
    next = fixPlacements(next)
    if (next !== orig) {
      await writeFile(f, next, 'utf8')
      updated++
      console.log(`  ${path.relative(ROOT, f)}`)
    }
  }
  console.log(`\n✓ ${updated} file(s) fixed.`)
}

main().catch((e) => { console.error(e); process.exit(1) })
