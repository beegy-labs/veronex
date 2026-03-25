# Frontend Review

> ADD Execution — Frontend Optimization & Policy Enforcement | **Last Updated**: 2026-03-24

## Trigger

User requests frontend code review, optimization, design token audit, i18n audit, or component refactor.

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Frontend patterns (SSOT) | `docs/llm/policies/patterns-frontend.md` | Always — contains all checklists |
| Design system (core) | `docs/llm/frontend/design-system.md` | Always |
| Component patterns | `docs/llm/frontend/design-system-components.md` | Component changes |
| i18n rules | `docs/llm/frontend/design-system-i18n.md` | i18n changes |
| Chart patterns | `docs/llm/frontend/charts.md` | Chart/analytics changes |
| Design tokens | `web/app/tokens.css` + `web/lib/design-tokens.ts` | Token compliance |
| i18n sources | `web/messages/en.json` + `ko.json` + `ja.json` | Key parity check |

> Checklist details (4-layer arch, design tokens, i18n, performance, TypeScript, a11y, fix priority) → `docs/llm/policies/patterns-frontend.md`

---

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Get the diff: `git diff HEAD` or read user-specified files |
| 2 | Launch 3 parallel review agents (Reuse · Quality · Efficiency) — pass full diff to each |
| 3 | Aggregate findings; discard false positives with reason |
| 4 | Fix violations P0 → P1 → P2 (see fix priority in `patterns-frontend.md`) |
| 5 | For i18n: update `en.json`, `ko.json`, `ja.json` simultaneously |
| 6 | Run `npx tsc --noEmit` — zero errors required |
| 7 | If N rounds requested: repeat steps 2–6 until N rounds consumed or no violations remain |
| 8 | CDD sync — update the relevant doc if a new pattern is established |

**Step 8 — which doc to update:**

| What changed | Target |
|--------------|--------|
| Token usage pattern | `docs/llm/policies/patterns-frontend.md` + `docs/llm/frontend/design-system.md` |
| New component pattern | `docs/llm/frontend/design-system-components.md` |
| i18n rule or key convention | `docs/llm/frontend/design-system-i18n.md` |
| Chart/formatter pattern | `docs/llm/frontend/charts.md` |
| Review rule (perf, a11y) | `docs/llm/policies/patterns-frontend.md` |

## Fix Iteration Policy

- Each *round* = one logical fix (a single coherent change)
- After every 3–4 rounds, run `tsc --noEmit` to catch regressions early
- False positives count as a round (document why the finding was skipped)
- Stop early if no remaining violations — do not manufacture changes to hit the count
- Parallel review agents always run **before** fixes begin, not interleaved
