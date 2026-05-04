# Design verodesign Sync

> ADD Execution — Sync With Upstream verodesign | **Last Updated**: 2026-05-03

## Trigger

verodesign upstream releases a new version. The `verodesign/packages/design/dist/css/` bundle and/or `themes/veronex.css` schema may have changed.

## Read Before Execution

| Doc | Path |
|-----|------|
| Design system | `docs/llm/frontend/design-system.md` (verodesign Token Architecture + Cascade gotcha) |
| Upstream theme | `verodesign/packages/design/dist/css/themes/veronex.css` |
| Local copy | `web/app/styles/vds/theme-veronex.css` |

## Files to Sync

| File | Source | Local | Edit Policy |
|------|--------|-------|-------------|
| Utility bundle | `verodesign/.../full.css` | `web/app/styles/vds/full.css` | Verbatim copy. Never hand-edit |
| State variants | `verodesign/.../state-variants.css` | `web/app/styles/vds/state-variants.css` | Verbatim copy |
| Responsive | `verodesign/.../responsive.css` | `web/app/styles/vds/responsive.css` | Verbatim copy |
| Animations | `verodesign/.../animations.css` | `web/app/styles/vds/animations.css` | Verbatim copy |
| Theme veronex | `verodesign/.../themes/veronex.css` | `web/app/styles/vds/theme-veronex.css` | Copy + apply selector patch + Verde Nexus value overrides |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Diff upstream vs local for each file under `web/app/styles/vds/` |
| 2 | For utility / state / responsive / animations: copy upstream verbatim |
| 3 | For `theme-veronex.css`: apply patches per "Theme Patch Recipe" below |
| 4 | Run `pnpm lint:no-tailwind` — must pass |
| 5 | Run `pnpm verify:contrast` — all 22 pairs must pass |
| 6 | Run `grep -rEho 'vds-[a-z-]+\[[^\]]+\]' app components` — confirm all arbitrary-value classes still have shims in `app/globals.css` (verodesign upstream may have started emitting some — drop those shims) |
| 7 | Rebuild docker container, Playwright probe in light + dark mode |
| 8 | Update `docs/llm/frontend/design-system.md` Last Updated + version pointer |

## Theme Patch Recipe

The local `theme-veronex.css` differs from upstream in two ways. Apply both after every sync.

| Patch | Purpose | How |
|-------|---------|-----|
| Descendant cascade selector | Work around `@property inherits: false` so tokens reach all DOM | Replace `[data-theme="veronex"]` with `[data-theme="veronex"], [data-theme="veronex"] *` on the token block |
| Verde Nexus value overrides | Apply current vN tuning (page bg, card bg, text ramp, status colors) | See `docs/llm/frontend/design-system.md` Verde Nexus table for current values |

## New Token Detection

| Step | Action |
|------|--------|
| 1 | Diff upstream `themes/veronex.css` vs local — list new `--vds-theme-*` lines |
| 2 | For each new token: decide whether to keep upstream default or override per Verde Nexus brand |
| 3 | If override: add to local file. If keep: still copy line over so descendant cascade applies |
| 4 | Mirror new tokens in `lib/design-tokens.ts` |
| 5 | If new text+bg pair: extend `verify-contrast.mjs` PAIRS |

## Removed Token Detection

| Step | Action |
|------|--------|
| 1 | Diff upstream vs local — list removed `--vds-theme-*` |
| 2 | Search consumer code: `grep -rn "vds-theme-<name>\|tokens\.<group>\.<name>" web/` |
| 3 | If usage found: replace with new equivalent before removing token |
| 4 | Remove from `lib/design-tokens.ts` and `verify-contrast.mjs` |

## Rules

| Rule | Detail |
|------|--------|
| Verbatim policy | Bundle CSS files (full/state-variants/responsive/animations) MUST be byte-identical to upstream. Local edits forbidden — they get clobbered on next sync |
| Patch documentation | Every diff vs upstream in `theme-veronex.css` MUST have a comment explaining why (cascade fix, brand override, eye-comfort tune) |
| Re-verify everything | Every sync = lint + contrast + visual probe in both modes. No exceptions |
| Atomic commit | All `vds/*.css` files updated in a single commit, with the upstream version pinned in commit message |

## Output Checklist

- [ ] All 5 `app/styles/vds/*.css` files synced
- [ ] Theme patches re-applied (selector + Verde Nexus values)
- [ ] New tokens added to `lib/design-tokens.ts` + `verify-contrast.mjs`
- [ ] Removed tokens replaced in consumer code
- [ ] `pnpm lint:no-tailwind` passes
- [ ] `pnpm verify:contrast` passes (22/22)
- [ ] Arbitrary-value shim list pruned for newly-supported classes
- [ ] Visual verification in light + dark via Playwright
- [ ] `design-system.md` Last Updated bumped
