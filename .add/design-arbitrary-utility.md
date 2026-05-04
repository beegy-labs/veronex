# Design Arbitrary Utility Add

> ADD Execution — Add `vds-*-[value]` Class | **Last Updated**: 2026-05-03

## Trigger

Need a per-pixel or off-scale utility class (`vds-h-[60px]`, `vds-sm:w-[560px]`, `vds-text-[11px]`, etc.) and the standard scale doesn't fit.

## Why This Workflow Exists

verodesign emits the standard scale only (`vds-h-9`, `vds-max-w-sm`, etc.). It does NOT emit per-pixel arbitrary value classes. A `vds-h-[60px]` in JSX silently renders as nothing — element falls back to default size, layout breaks (sidebar header collapses, floating panel goes full-width, modals overflow).

## Decide Path

| Available | Action |
|-----------|--------|
| Standard scale fits (`vds-h-15` = 60px, `vds-max-w-xl` = 576px, etc.) | Use standard. STOP here |
| User pinned exact off-scale value | Add to override layer per below |
| Constraint is design-token-shaped (color/spacing rhythm) | Open new token via `.add/design-token-add.md` instead |

## Read Before Execution

| Doc | Path |
|-----|------|
| Design system | `docs/llm/frontend/design-system.md` (Migration audit row "Arbitrary-value utilities") |
| Override file | `web/app/globals.css` (overrides layer, end of file) |

## Files to Edit

| File | Edit |
|------|------|
| `web/app/globals.css` | Append `.vds-<name>\[<value>\] { <css>: <value>; }` in arbitrary-value block |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Confirm no standard-scale class fits |
| 2 | Choose CSS class with proper escapes — `[`, `]`, `%`, `.`, `:` need backslash escape in selector |
| 3 | Append rule to arbitrary-value block in `app/globals.css` |
| 4 | If responsive variant (`vds-sm:`, `vds-md:`), wrap in matching `@media` |
| 5 | Run `grep -rEho 'vds-[a-z-]+\[[^\]]+\]' app components` — confirm new class is in code |
| 6 | Rebuild docker container, verify via Playwright |

## Escape Cheatsheet

| Char in Value | Selector Escape | Example |
|---------------|-----------------|---------|
| `[` `]` | `\[` `\]` | `.vds-h-\[60px\]` |
| `%` | `\%` | `.vds-max-w-\[40\%\]` |
| `.` | `\.` | `.vds-tracking-\[0\.3em\]` |
| `:` (state) | `\:` | `.vds-sm\:w-\[560px\]` |

## Existing Catalog

Maintained in `app/globals.css`. Keep IN SYNC. Discover usage:

```
grep -rEho 'vds-[a-z-]+\[[^\]]+\]' app components | sort -u
```

Currently shimmed: heights (60/8/520px), min/max heights (64/80px, 70-90vh), max-widths (120/160/180/300px, 40/80%/95vw), widths (20/38%, sm:560px), text sizes (10-12px), tracking (0.3em), z-index (9999).

## Rules

| Rule | Detail |
|------|--------|
| Override layer only | Place rules inside `@layer overrides { ... }` so they sit after vds-utilities |
| Responsive wrap | `vds-md:foo-[bar]` rule MUST be inside `@media (min-width: 768px)` block |
| No new sizing rhythms | Repeated sizes (h-72, h-96 etc.) belong in a token, not arbitrary value |
| Comment block tag | The arbitrary-value section in `globals.css` has a header comment — add new entries there, not scattered |
| Sweep on PR | Reviewer runs `grep -rEho 'vds-[a-z-]+\[[^\]]+\]'` to confirm every used class has a rule |

## Output Checklist

- [ ] Standard scale checked first; no good fit
- [ ] Rule added to `app/globals.css` arbitrary-value block
- [ ] Selector escapes correct
- [ ] Responsive variants in matching `@media`
- [ ] Visual verification: element renders at expected size
