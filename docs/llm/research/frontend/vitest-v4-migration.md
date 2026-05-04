# Vitest v4 Migration Notes

> Research | **Last Updated**: 2026-04-22
> Historical migration reference. Live policy → `policies/testing-strategy.md`.

## Pool options moved to top level

`poolOptions` removed — hoist to top level.

```ts
// v3
poolOptions: { threads: { maxThreads: 4, singleThread: true } }

// v4
maxWorkers: 4,
isolate: false,   // replaces singleThread
```

## Environment assignment via `projects`

`environmentMatchGlobs` removed — express as a `projects` entry.

```ts
// v3
environmentMatchGlobs: [['**/*.spec.ts', 'jsdom']]

// v4
projects: [
  { test: { include: ['**/*.spec.ts'], environment: 'jsdom' } },
]
```

## Test options argument position

Options argument moved from 3rd to 2nd.

```ts
// v3
test('name', () => {}, { retry: 2 })

// v4
test('name', { retry: 2 }, () => {})
```

## `done` callback removed

Use `async`/`await`:

```ts
// v3
test('async', (done) => { done() })

// v4
test('async', async () => { await something() })
```

## Mock behavior changes

- `vi.restoreAllMocks()` no longer resets `vi.fn()` — add `vi.clearAllMocks()` explicitly if needed
- Mock default name changed from `'spy'` → `'vi.fn()'` — update any snapshots asserting on mock names
- Module factory must return an export object: `vi.mock('./x', () => ({ default: 'val' }))` (not bare value)

## Reporter changes

`basic` reporter removed. Use `{ reporter: 'default', summary: false }`.

## Minimum requirements

- Node.js ≥ 20
- Vite ≥ 6
