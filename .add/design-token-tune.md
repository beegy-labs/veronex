# Design Token Tune

> ADD Execution — Adjust Existing Token Values | **Last Updated**: 2026-05-03

## Trigger

User reports color/contrast/comfort issue. Examples: "card too dark", "text hard to read", "dark mode washed out", "eye strain".

## Read Before Execution

| Doc | Path | Why |
|-----|------|-----|
| Design system | `docs/llm/frontend/design-system.md` | Current Verde Nexus values + dark-mode iteration history |
| Theme file | `web/app/styles/vds/theme-veronex.css` | Where to edit |

## Diagnose First

| Symptom | Likely Cause | Fix Direction |
|---------|--------------|---------------|
| Card indistinguishable from page | bg-card / bg-page L gap < 3% | Widen L gap to >= 4-6% |
| Text "muddy" or "tired" | text-dim / text-secondary L < 70 in dark | Bump L by 8-12pp |
| Pure white text fatigue | text-primary L >= 98 + chroma 0 | Cap at L96, add chroma 0.005 hue 150 |
| Pure black bg halation | bg-page L < 6 in dark | Lift to L7-10 |
| Surface looks "themed/colored" | Surface chroma > 0.020 | Reduce chroma to <= 0.012 |
| Surface looks "flat gray/dim" | Surface chroma 0 | Add slight chroma 0.005-0.014 |
| Status accent "dead" | Dark accent chroma < 0.15 | Boost to 0.16-0.20 |
| User pins exact hex | Match exactly | Use hex literal in `light-dark()` |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Identify failing token via Playwright probe of `getComputedStyle(...)` |
| 2 | Decide new value per Diagnose table; preserve Verde Nexus hue family (150) where applicable |
| 3 | Edit `theme-veronex.css` descendant-cascade block ONLY |
| 4 | Run `pnpm verify:contrast` — all 22 pairs must still pass |
| 5 | Rebuild + restart docker container |
| 6 | Visual verification in both modes via Playwright |
| 7 | Update `docs/llm/frontend/design-system.md` Verde Nexus table if changed |

## Rules

| Rule | Detail |
|------|--------|
| Preserve brand | primary stays Deep Ivy / Bio-Emerald (`oklch(30% 0.06 150)` / `oklch(72% 0.16 165)`). Never change without explicit user approval |
| User-pinned values | If user provides exact hex/HSL/RGB, match exactly. Document in comment alongside |
| WCAG floor | text-primary AAA, body AAA, dim/faint AA-large, status AA, focus UI |
| Iteration log | Append `/* vN — <reason> */` comment block when bumping major version (current: v6 readability pass) |
| Eye health | Dark page L >= 7, dark text-primary L <= 96, hue family aligned with brand to avoid clash |
| Cascade | Edit happens inside `[data-theme="veronex"], [data-theme="veronex"] *` block; never `:root` only |

## Common Adjustments

| Goal | Token | Typical Move |
|------|-------|--------------|
| Premium "Linear/Vercel" dark | bg-page, bg-card | Pure neutral (chroma <= 0.005) at L7/L11; let accents pop |
| Premium "GitHub Dark" slate | bg-card | Slate blue chroma 0.014 hue 240 at L13-18 |
| Premium "Verde Nexus" green-tint | bg-card | Hue 150 chroma 0.005 at L13-18 (current v6 = `#101412`) |
| Improve dim-text readability | text-dim, text-secondary, text-faint | +8-12pp L; keep slight hue chroma 0.005-0.008 |
| Status accent vibrancy | success/warning/error dark | Chroma 0.16 -> 0.18-0.20 |

## Output Checklist

- [ ] Token edited in `theme-veronex.css` descendant-cascade block
- [ ] `pnpm verify:contrast` passes (22/22)
- [ ] Visual verification in light + dark via Playwright
- [ ] Iteration comment updated if version bumped
- [ ] `design-system.md` Verde Nexus table reflects new values
