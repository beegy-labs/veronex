# Design Token Add

> ADD Execution — Add `--vds-theme-*` Override | **Last Updated**: 2026-05-03

## Trigger

User asks to add a new color, surface, status, or accent token to the Verde Nexus theme.

## Read Before Execution

| Doc | Path | Why |
|-----|------|-----|
| Design system | `docs/llm/frontend/design-system.md` | Token architecture + cascade gotcha |
| Token policy | `docs/llm/policies/patterns-frontend/tokens.md` | Naming + layering rules |
| verodesign source | `verodesign/packages/design/dist/css/themes/veronex.css` | Upstream token list (do not edit) |

## Files to Edit

| File | Edit |
|------|------|
| `web/app/styles/vds/theme-veronex.css` | Append `--vds-theme-<name>: light-dark(<light>, <dark>);` inside `[data-theme="veronex"], [data-theme="veronex"] *` block |
| `web/lib/design-tokens.ts` | Add `<name>: 'var(--vds-theme-<name>)'` under matching group |
| `web/scripts/verify-contrast.mjs` | If text+bg pair, append to `PAIRS` array with WCAG level |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Decide name — match existing taxonomy (`bg-*`, `text-*`, `border-*`, `primary*`, `accent-*`, status, chart-N) |
| 2 | Pick OKLCH values — see Value Rules below; both light + dark required |
| 3 | Append to `theme-veronex.css` inside the descendant-cascade block (NOT `:root` only — see cascade gotcha) |
| 4 | Mirror in `lib/design-tokens.ts` for type-safe TSX/SVG access |
| 5 | If text+bg pair → extend `PAIRS` in `verify-contrast.mjs` |
| 6 | Run `pnpm verify:contrast` — must show 22+/22+ pass |
| 7 | Run `pnpm lint:no-tailwind` — must show 0 hits |
| 8 | Rebuild docker container, verify via Playwright in light + dark mode |

## Value Rules

| Rule | Detail |
|------|--------|
| Format | `light-dark(oklch(L% C H), oklch(L% C H))` — both required, lightningcss compiles to `var()` pair |
| Hue family | Verde Nexus = 150 (green); cool surfaces = 220-240; status = success 145, warning 70, error/destructive 25, info 165 |
| Light surface chroma | <= 0.012 (avoid muddy backgrounds) |
| Dark surface chroma | <= 0.018 (slate without crossing into "themed") |
| Accent chroma | 0.15-0.21 (let status pop against neutral surfaces) |
| Text chroma | <= 0.012 (slight tint OK; avoid color cast) |
| Min luminance gap | bg-page vs bg-card >= 4% L; bg-card vs text >= AAA contrast |
| Hex literals | Allowed when matching exact user-pinned color (e.g. `#101412` for bg-card). Comment with HSL for clarity |

## Cascade Gotcha

verodesign declares `@property --vds-theme-* { inherits: false; }`. Tokens declared only on `:root` do NOT reach descendants. Always declare overrides on:

```css
[data-theme="veronex"],
[data-theme="veronex"] * {
  --vds-theme-<name>: light-dark(...);
}
```

Verified by Playwright probe after every theme edit.

## Rules

| Rule | Detail |
|------|--------|
| Single source | Token values live ONLY in `theme-veronex.css`. Never inline `var(--vds-theme-*)` strings or hex in TSX |
| Status mode-aware | A single OKLCH cannot satisfy WCAG AA on both light card (L100) and dark card (L18). Status tokens MUST use `light-dark()` with mode-specific values |
| Foreground pair | Every solid bg token (primary, accent, status) needs a `-fg` companion that hits >= 4.5:1 |
| Eye comfort | Dark text-primary <= L96 (avoid pure white halation per Bartlett 2017). Dark page bg >= L7 (avoid pure black halation) |
| WCAG enforcement | Fail = block. Run `pnpm verify:contrast` after any token change |

## Output Checklist

- [ ] Token in `theme-veronex.css` descendant-cascade block
- [ ] Mirrored in `lib/design-tokens.ts`
- [ ] If text+bg pair: added to `verify-contrast.mjs` PAIRS
- [ ] `pnpm verify:contrast` passes
- [ ] `pnpm lint:no-tailwind` clean
- [ ] Visual verification in light + dark via Playwright
