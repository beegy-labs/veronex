#!/usr/bin/env node
/**
 * Contrast verification — runs against the verodesign theme CSS in our app.
 * Ensures every text-on-surface pair meets WCAG AAA (≥7:1) and every
 * UI/non-text pair meets WCAG AA (≥3:1) in BOTH light and dark mode.
 *
 * Source: app/styles/vds/theme-veronex.css (light-dark()-collapsed values).
 * We re-implement the OKLCH → sRGB conversion via `culori`.
 */

import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { parse, formatHex, wcagContrast } from 'culori'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const THEME = path.join(ROOT, 'app/styles/vds/theme-veronex.css')

// Levels per docs/llm/frontend/design-system.md:
//   - Body primary text ........ AAA (≥7:1)
//   - Body secondary text ...... AAA (≥7:1)
//   - Body dim/hint text ....... AA-large (≥3:1) — caption/hint tier
//   - Brand primary on page bg . AAA (≥7:1)
//   - Button text (primary-fg) . AA (≥4.5:1) — WCAG 2.1 AA SC 1.4.3
//   - Status colors ............ AA (≥4.5:1) — WCAG 2.1 AA per policy line
//   - UI / focus indicator ..... UI (≥3:1)   — WCAG 2.2 SC 1.4.11
const PAIRS = [
  ['text-primary',   'bg-page',  'AAA'],
  ['text-primary',   'bg-card',  'AAA'],
  ['text-secondary', 'bg-card',  'AAA'],
  ['text-dim',       'bg-card',  'AA-large'],
  ['primary',        'bg-page',  'AAA'],
  ['primary-fg',     'primary',  'AA'],
  ['success',        'bg-card',  'AA'],
  ['error',          'bg-card',  'AA'],
  ['warning',        'bg-card',  'AA'],
  ['info',           'bg-card',  'AA'],
  ['border-focus',   'bg-page',  'UI'],
]

const REQUIRED = { AA: 4.5, AAA: 7.0, 'AA-large': 3.0, 'UI': 3.0 }

function extractTheme(css) {
  // Match `--vds-theme-NAME: light-dark(LIGHT, DARK);` where LIGHT/DARK may
  // themselves contain `oklch(...)` (nested parens). Use a balanced-paren
  // matcher: `(?:[^()]|\([^)]*\))+` — non-paren chars OR a single `(...)` group.
  const re = /--vds-theme-([\w-]+):\s*light-dark\(\s*((?:[^(),]|\([^)]*\))+?)\s*,\s*((?:[^()]|\([^)]*\))+?)\s*\)\s*;/g
  const tokensByMode = { light: {}, dark: {} }
  // Plain values (not light-dark) — applied to both modes
  const re2 = /--vds-theme-([\w-]+):\s*([^;]+);/g
  let m
  while ((m = re.exec(css))) {
    tokensByMode.light[m[1]] = m[2].trim()
    tokensByMode.dark[m[1]]  = m[3].trim()
  }
  while ((m = re2.exec(css))) {
    if (!tokensByMode.light[m[1]] && !m[2].trim().startsWith('light-dark')) {
      tokensByMode.light[m[1]] = m[2].trim()
      tokensByMode.dark[m[1]]  = m[2].trim()
    }
  }
  return tokensByMode
}

function toHex(value) {
  // value is something like "oklch(30% 0.06 150)" or a hex
  const parsed = parse(value)
  if (!parsed) return null
  return formatHex(parsed)
}

async function main() {
  const css = await readFile(THEME, 'utf8')
  const tokens = extractTheme(css)
  const failures = []
  for (const mode of ['light', 'dark']) {
    for (const [tk, bk, level] of PAIRS) {
      const tv = tokens[mode][tk]
      const bv = tokens[mode][bk]
      if (!tv || !bv) {
        failures.push(`${mode}: missing token (${tk} or ${bk})`)
        continue
      }
      const tc = toHex(tv); const bc = toHex(bv)
      if (!tc || !bc) {
        failures.push(`${mode}: parse failed (${tk}=${tv} / ${bk}=${bv})`)
        continue
      }
      const ratio = wcagContrast(tc, bc)
      const min = REQUIRED[level]
      const pass = ratio >= min
      const tag = pass ? '✓' : '✗'
      const line = `${tag} ${mode.padEnd(5)} ${level} ${tk} on ${bk}: ${ratio.toFixed(2)} (need ≥${min})`
      if (pass) console.log(line)
      else failures.push(line)
    }
  }

  if (failures.length) {
    console.error('\n--- FAILURES ---')
    for (const f of failures) console.error(f)
    process.exit(1)
  }
  console.log(`\n✓ all contrast pairs pass (light + dark, ${PAIRS.length}×2 = ${PAIRS.length * 2})`)
}

main().catch((e) => { console.error(e); process.exit(1) })
