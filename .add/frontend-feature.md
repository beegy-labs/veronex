# Frontend Feature Addition

> ADD Execution — New Page / Feature | **Last Updated**: 2026-04-22

## Trigger

User requests a new frontend page, feature, or domain UI.

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Execution contracts | `docs/llm/frontend/execution-contracts.md` | Always — folder structure + naming |
| Frontend patterns (SSOT) | `docs/llm/policies/patterns-frontend.md` | Always |
| Design system | `docs/llm/frontend/design-system.md` | Always |
| Component patterns | `docs/llm/frontend/design-system-components.md` | Component changes |
| Extended patterns | `docs/llm/frontend/design-system-components-patterns.md` | Modals, mutations, 2-step verify |
| i18n rules | `docs/llm/frontend/design-system-i18n.md` | Always |
| Charts | `docs/llm/frontend/charts.md` | Chart/analytics features |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Read `execution-contracts.md` — determine feature folder location |
| 2 | Add TypeScript types to `web/lib/types.ts` + Zod schema if untrusted API data |
| 3 | Add API functions to `web/lib/api.ts` (object pattern, no raw fetch) |
| 4 | Add `queryOptions()` factory to `web/lib/queries/{domain}.ts` (SSOT for queryKey + staleTime) |
| 5 | Create page: `web/app/{route}/page.tsx` — `'use client'` + `useQuery(domainQuery)` + layout only |
| 6 | Create feature components under `web/app/{route}/components/` — no business logic in shared `components/` |
| 7 | Add `usePageGuard(menuId)` to page |
| 8 | Add nav entry to `web/components/nav.tsx` if needed |
| 9 | Add i18n keys: `en.json` → `ko.json` → `ja.json` simultaneously |
| 10 | Run `npx tsc --noEmit` — zero errors |
| 11 | Write tests per `.add/frontend-test.md` — pick layer via checklist; never duplicate across layers |
| 12 | Run frontend review: `.add/frontend-review.md` |

## Rules

| Rule | Detail |
|------|--------|
| No direct fetch | All data fetching via `queryOptions()` + `useQuery` — never raw `fetch()` or `useState + useEffect` |
| No inline queryKey | All query keys via `QUERY_KEYS` constants in `lib/queries/` |
| Error handling | `ApiHttpError instanceof` only — never `(e as any).status` |
| Mutations | `useApiMutation` for mutations needing cache invalidation |
| Optimistic UI | `useOptimistic` on toggle/switch mutations |
| i18n | All user-facing strings via `t()` — no hardcoded strings |
| Feature components | Page-specific components in `app/{route}/components/` — never in `components/` |
| Shared components | Only generic, reusable components go in `components/`. Single-importer shared = move to feature |
| 4-layer | Pages → Feature components → Shared components → Foundation (lib/hooks/queries) |
| Color | All colors via `tokens.css` SSOT. Change = Layer 1 + Layer 2 only. Zero color edits in `.tsx` |
| No Atomic Design | Do not create `components/atoms/`, `molecules/`, `organisms/`, `templates/`. Do not use atom/molecule/organism in names or reviews. See `patterns-frontend/architecture.md § 4-Layer Component Architecture / Non-Goals` |

## Output Checklist

- [ ] Types in `lib/types.ts`
- [ ] API function in `lib/api.ts`
- [ ] `queryOptions()` in `lib/queries/{domain}.ts`
- [ ] Page with `usePageGuard`
- [ ] Feature components isolated in `app/{route}/components/`
- [ ] i18n keys in all 3 locales
- [ ] `tsc --noEmit` passes
- [ ] Frontend review passed
