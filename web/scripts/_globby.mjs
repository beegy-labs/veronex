/**
 * Tiny dependency-free file collector. Pass an array of {dir, extensions} or
 * a flat list of root dirs and a single extension list.
 */
import { readdir } from 'node:fs/promises'
import path from 'node:path'

async function walk(dir) {
  const out = []
  let entries
  try {
    entries = await readdir(dir, { withFileTypes: true })
  } catch { return [] }
  for (const e of entries) {
    if (e.name === 'node_modules' || e.name.startsWith('.')) continue
    const full = path.join(dir, e.name)
    if (e.isDirectory()) out.push(...(await walk(full)))
    else if (e.isFile()) out.push(full)
  }
  return out
}

/** Collect files under any of `roots` whose extension is in `exts` (e.g. ['.tsx', '.ts']). */
export async function collect(roots, exts) {
  const files = []
  for (const r of roots) files.push(...(await walk(r)))
  return files.filter((f) => exts.some((ext) => f.endsWith(ext))).sort()
}

// Backwards-compat shim used by tw-to-vds.mjs
export async function globby(patterns) {
  const roots = new Set()
  const exts = new Set()
  for (const p of patterns) {
    const star = p.indexOf('*')
    const root = star === -1 ? path.dirname(p) : p.slice(0, star).replace(/\/$/, '')
    roots.add(root)
    const dotIdx = p.lastIndexOf('.')
    if (dotIdx > star) exts.add(p.slice(dotIdx))
  }
  return collect([...roots], [...exts])
}
