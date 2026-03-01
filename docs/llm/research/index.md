# Research — 2026 Best Practices

> **Tier 2 CDD** | Editable | Last Updated: 2026-03-02
>
> Web-searched, implementation-verified findings.
> Each file records **what** was decided, **why**, and **where** it is used in this codebase.

---

## How to Use

1. **Before implementing** a new feature, read the relevant file here first.
2. **After a web search** on a technical topic, append findings to the appropriate file.
3. **After verifying** in production, upgrade status from 🔬 to ✅.

### Status Legend

| Icon | Meaning |
|------|---------|
| ✅ | Verified — used in this codebase, production-tested |
| 🔬 | Research-only — found via web search, not yet implemented here |
| 📋 | Placeholder — to be researched |

---

## Frontend (`frontend/`)

| File | Topics | Status |
|------|--------|--------|
| `frontend/css-animations.md` | CSS Motion Path, offset-path, SMIL, particle systems, keyframes | ✅ |
| `frontend/react.md` | useReducer, ResizeObserver, onAnimationEnd, state patterns | ✅ |
| `frontend/data-fetching.md` | TanStack Query v5, polling, background refetch, staleTime | ✅ |
| `frontend/nextjs.md` | App Router, 'use client' rationale, Server Actions, PPR, Suspense | ✅ |
| `frontend/tailwind.md` | Tailwind v4 CSS-first, 4-layer tokens, @utility, container queries | ✅ |
| `frontend/tanstack-query.md` | queryOptions factory, lib/queries/ SSOT, invalidation, optimistic updates | ✅ |

## Backend (`backend/`)

| File | Topics | Status |
|------|--------|--------|
| `backend/rust-axum.md` | Axum 0.8, tokio, path params, SSE, middleware, AppState | ✅ |
| `backend/api-design.md` | REST design, versioning, OpenAPI 3.1, rate limit headers, pagination | ✅ |

## Infrastructure (`infrastructure/`)

| File | Topics | Status |
|------|--------|--------|
| `infrastructure/observability.md` | OTel pipeline, Redpanda, ClickHouse, collector config | ✅ |
| `infrastructure/database.md` | PostgreSQL 18, ClickHouse, migrations, uuidv7 | 🔬 |

## Security (`security/`)

| File | Topics | Status |
|------|--------|--------|
| `security/auth.md` | JWT, sessions, refresh tokens, revocation, BLAKE2b | ✅ |

---

## Quick Reference

| Need to implement... | Read |
|----------------------|------|
| Animated particles / visualization | `frontend/css-animations.md` |
| Complex UI state (reducers, cleanup) | `frontend/react.md` |
| Polling / background data sync | `frontend/data-fetching.md` |
| TanStack Query queryOptions / invalidation | `frontend/tanstack-query.md` |
| Tailwind v4 tokens / custom utilities | `frontend/tailwind.md` |
| Next.js page architecture decision | `frontend/nextjs.md` |
| New Axum handler or middleware | `backend/rust-axum.md` |
| API endpoint design / OpenAPI | `backend/api-design.md` |
| OTel metrics or traces | `infrastructure/observability.md` |
| Auth / JWT session management | `security/auth.md` |
