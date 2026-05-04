# Frontend Patterns — TypeScript, Zod & UI State Types

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

## TypeScript + Zod (API Boundary Validation)

TypeScript enforces compile-time types; Zod validates untrusted API responses at runtime.

```typescript
// web/lib/types.ts
import { z } from 'zod'

export const ProviderSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  provider_type: z.enum(['ollama', 'gemini']),
  status: z.enum(['online', 'offline', 'degraded']),
  is_active: z.boolean(),
})
export type Provider = z.infer<typeof ProviderSchema>

// safeParse for graceful error handling (no throws)
const result = ProviderSchema.safeParse(apiResponse)
if (!result.success) console.error(result.error.issues)

// Branded types prevent wrong-ID bugs
const ProviderIdSchema = z.string().uuid().brand<'ProviderId'>()
type ProviderId = z.infer<typeof ProviderIdSchema>
```

Apply Zod at entry points: API responses, form inputs, env vars.

### FlowStats — Server-Computed Rates

`FlowStatsSchema` fields: `incoming` (10s window count), `incoming_60s` (60s window count = req/m), `queued`, `running`, `completed`. All `NonNegativeInt`.

- `req/s` = `incoming / 10` (client divides)
- `req/m` = `incoming_60s` (server-computed 60-bucket sliding window, NOT `req/s * 60`)
- Server broadcasts every second unconditionally — clients rely on this cadence

---

## UI-State Types in `web/lib/types.ts`

Modal/form state types that appear across multiple components belong in `web/lib/types.ts`, not as local `type` definitions.

```typescript
// web/lib/types.ts
export type VerifyState = 'idle' | 'checking' | 'ok' | 'error'
```

Import in components: `import type { VerifyState } from '@/lib/types'`

Rule: if the same `type Foo = 'a' | 'b' | ...` appears in 2+ component files, move it to `lib/types.ts`.

---

## HTTP Errors with Status Code (`ApiHttpError`)

Custom fetch helpers that need to distinguish HTTP status codes throw `ApiHttpError` from `web/lib/types.ts`:

```typescript
// lib/api.ts — throwing
import { ApiHttpError } from './types'
if (!res.ok) throw new ApiHttpError(data.error ?? `${res.status}`, res.status)

// Component onError — handling
import { ApiHttpError } from '@/lib/types'
onError: (e) => {
  const msg = e instanceof ApiHttpError && e.status === 409
    ? t('...duplicateUrl')
    : (e instanceof Error ? e.message : t('...connectionFailed'))
}
```

Rule: never cast `(e as Error & { status?: number })` — use `instanceof ApiHttpError` instead.

---

## TypeScript Strictness

| Rule | Detail |
|------|--------|
| No `any` | Replace with proper type or `unknown` + type guard |
| Non-null `!` | Replace with optional chaining or explicit null check where possible |
| Generated types | Use types from `web/lib/generated/` — never redefine domain enums locally |
| Zod at boundaries | Parse API responses at `lib/api.ts` — components receive typed data |
| UI state types | `type Foo = 'a' \| 'b' \| ...` shared across 2+ files → move to `lib/types.ts` |

---

