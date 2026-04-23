# Code Patterns: Frontend — Index

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Next.js 16 · React 19 · TanStack Query v5 · Tailwind v4 · Zod
> Rust patterns → `policies/patterns.md`
> Full rule text lives in `patterns-frontend/{domain}.md`; this file is an index.

## Index by Domain

| Domain | File | Covers |
|---|---|---|
| TanStack Query v5 (core) | [`patterns-frontend/queries.md`](patterns-frontend/queries.md) | queryOptions factory, mutationOptions, withJitter, object-vs-factory, timing constants |
| TanStack Query v5 (advanced) | [`patterns-frontend/queries-advanced.md`](patterns-frontend/queries-advanced.md) | useSuspenseQuery, skipToken, useMutationState, streamedQuery, isEnabled, query key constants, onSettled |
| React 19/19.2 + Compiler + Performance | [`patterns-frontend/react.md`](patterns-frontend/react.md) | useOptimistic, React Compiler, useMemo, React 19.2 (Activity / useEffectEvent), performance rules |
| TypeScript, Zod & UI State Types | [`patterns-frontend/types.md`](patterns-frontend/types.md) | Zod boundary validation, UI-state types SSOT, ApiHttpError, TypeScript strictness |
| Design Tokens & Chart Theme | [`patterns-frontend/tokens.md`](patterns-frontend/tokens.md) | Shared style constants, chart tooltip style, chart formatters, Design Token 4-Layer Architecture |
| E2E Test Patterns | [`patterns-frontend/testing.md`](patterns-frontend/testing.md) | Playwright E2E patterns, test constants, resource cleanup, API auth helper, ConfirmDialog interaction |
| UI, Icons & Accessibility | [`patterns-frontend/ui.md`](patterns-frontend/ui.md) | SVG pattern IDs, lucide-react v1 (LucideProvider / aria-hidden / createLucideIcon / DynamicIcon), WCAG a11y |
| 4-Layer Architecture, Pages, Prefetch | [`patterns-frontend/architecture.md`](patterns-frontend/architecture.md) | 4-Layer component architecture, Adding a new page, Page Guard, Query Prefetch, Historical data caching |
| i18n Compliance | [`patterns-frontend/i18n.md`](patterns-frontend/i18n.md) | i18n compliance rules |

## Section Location (cross-ref resolution)

When a rule references `patterns-frontend.md § X`, use this table to locate the full text.

| § Section | File |
|---|---|
| TanStack Query v5 | `patterns-frontend/queries.md` |
| `queryOptions()` Factory -- SSOT Pattern | `patterns-frontend/queries.md` |
| Query Timing Constants | `patterns-frontend/queries.md` |
| `withJitter()` — Polling Storm Prevention | `patterns-frontend/queries.md` |
| `queryOptions()` — Object vs Factory Function | `patterns-frontend/queries.md` |
| `mutationOptions()` Factory (v5.82+) | `patterns-frontend/queries.md` |
| `useSuspenseQuery` — Data-Guaranteed Rendering | `patterns-frontend/queries-advanced.md` |
| `skipToken` — Conditional Queries (TypeScript-idiomatic) | `patterns-frontend/queries-advanced.md` |
| `useMutationState` — Cross-Component Mutation Observation | `patterns-frontend/queries-advanced.md` |
| `experimental_streamedQuery` — SSE Streaming Queries | `patterns-frontend/queries-advanced.md` |
| `isEnabled` Return Value (v5.83+) | `patterns-frontend/queries-advanced.md` |
| Query Key Constants — Invalidation SSOT | `patterns-frontend/queries-advanced.md` |
| Inline `useQuery` (one-off, modal-only fetches) | `patterns-frontend/queries-advanced.md` |
| Mutation -- `onSettled` for cache invalidation | `patterns-frontend/queries-advanced.md` |
| React 19 -- useOptimistic | `patterns-frontend/react.md` |
| TypeScript + Zod (API Boundary Validation) | `patterns-frontend/types.md` |
| Shared Style Constants | `patterns-frontend/tokens.md` |
| Chart Tooltip Style | `patterns-frontend/tokens.md` |
| Chart Theme Formatters | `patterns-frontend/tokens.md` |
| React Compiler (v1.0, October 2025) | `patterns-frontend/react.md` |
| useMemo for Derived Data | `patterns-frontend/react.md` |
| Design Token System (4-Layer Architecture) | `patterns-frontend/tokens.md` |
| E2E Test Patterns | `patterns-frontend/testing.md` |
| UI-State Types in `web/lib/types.ts` | `patterns-frontend/types.md` |
| HTTP Errors with Status Code (`ApiHttpError`) | `patterns-frontend/types.md` |
| SVG Pattern IDs — `useId()` for DOM Uniqueness | `patterns-frontend/ui.md` |
| Query Prefetch in AppShell | `patterns-frontend/architecture.md` |
| Historical Data — `STALE_TIME_HISTORY` | `patterns-frontend/architecture.md` |
| Page Guard (`usePageGuard`) | `patterns-frontend/architecture.md` |
| Adding a New Page | `patterns-frontend/architecture.md` |
| 4-Layer Component Architecture | `patterns-frontend/architecture.md` |
| i18n Compliance | `patterns-frontend/i18n.md` |
| Performance Rules | `patterns-frontend/react.md` |
| TypeScript Strictness | `patterns-frontend/types.md` |
| Accessibility — WCAG 2.1 AA (Admin Dashboard Scope) | `patterns-frontend/ui.md` |
| lucide-react v1 Patterns | `patterns-frontend/ui.md` |
| React 19.2 Patterns | `patterns-frontend/react.md` |

## Review Fix Priority

| Priority | Category |
|----------|----------|
| P0 (fix immediately) | Hardcoded hex, wrong token names, broken i18n keys, missing i18n parity |
| P1 (fix in same pass) | Raw `var(--theme-*)` strings, missing `useMemo`, missing `aria-label`, SSE components without `React.memo`, time-display without interval tick, `onSuccess` for invalidation (→ `onSettled`), icon-only semantic icons missing `aria-hidden={false}` |
| P2 (fix if touching file) | Component extraction for 3+ duplicates, prop count reduction, zero-value stat containers |
