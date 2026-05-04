# Design Contrast Audit

> ADD Execution — Run WCAG Contrast Verification | **Last Updated**: 2026-05-03

## Trigger

Any change to `theme-veronex.css`, before merging a design-token PR, or when a user reports legibility issues.

## Read Before Execution

| Doc | Path |
|-----|------|
| Design system | `docs/llm/frontend/design-system.md` (Verde Nexus values + WCAG targets) |
| Script | `web/scripts/verify-contrast.mjs` |

## Pair Coverage

22 pairs across light + dark. Source of truth = `PAIRS` array in script.

| Pair | Required Level | Why |
|------|----------------|-----|
| text-primary on bg-page | AAA (>=7:1) | Body text |
| text-primary on bg-card | AAA | Body text on elevated surface |
| text-secondary on bg-card | AAA | Subheadings, sub-labels |
| text-dim on bg-card | AA-large (>=3:1) | Captions, hints |
| primary on bg-page | AAA | Brand surface emphasis |
| primary-fg on primary | AA (>=4.5:1) | Button text |
| success / error / warning / info on bg-card | AA | Status text/icons |
| border-focus on bg-page | UI (>=3:1) | WCAG 2.2 SC 1.4.11 focus indicator |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Run `pnpm verify:contrast` from `web/` |
| 2 | Read output — every line must show `✓` |
| 3 | Final line must be: `✓ all contrast pairs pass (light + dark, 11×2 = 22)` |
| 4 | If any `✗`: re-tune the failing token via `.add/design-token-tune.md` |

## Common Failure Patterns

| Failure | Fix |
|---------|-----|
| Status on bg-card fails AA in one mode | Make token mode-aware via `light-dark()` — different L per mode |
| text-dim fails AA-large | Bump dark text-dim L by 8-12pp (typical: 62 -> 74) |
| primary-fg on primary fails AA | Recompute fg L: dark mode primary at L72 needs fg at L15 (oklch(15% 0.04 150)) |
| New token added but pair not checked | Add pair to `PAIRS` in `verify-contrast.mjs` |

## Adding a New Pair

| Step | Action |
|------|--------|
| 1 | Open `web/scripts/verify-contrast.mjs` |
| 2 | Append to `PAIRS`: `['<text-token>', '<bg-token>', '<level>']` |
| 3 | Levels: `'AAA'` 7.0, `'AA'` 4.5, `'AA-large'` 3.0, `'UI'` 3.0 |
| 4 | Re-run `pnpm verify:contrast` |
| 5 | Update count in success message string if needed |

## Rules

| Rule | Detail |
|------|--------|
| Block on fail | Never merge a theme PR with a failing pair |
| Mode parity | Both light + dark must pass same level |
| No exception list | If a pair fails, fix the token — do not add to ignore list |
| Headless verify | Script reads `theme-veronex.css` directly (no browser); cheap to run repeatedly |

## Output Checklist

- [ ] `pnpm verify:contrast` exits 0
- [ ] Output shows 22+/22+ pass (light + dark)
- [ ] If new tokens added, PAIRS extended
