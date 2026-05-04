import { defineConfig } from 'vitest/config'
import path from 'path'

// Multi-project layout mirrors docs/llm/policies/testing-strategy.md § Layer Responsibility.
// Run a single layer: `npx vitest run --project <name>`
//   unit        — pure function / hook logic in node
//   integration — API schemas + cross-module wiring (node)
//   component   — single-component render + interaction in a real browser (scaffolded)
export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, '.'),
    },
  },
  test: {
    globals: true,
    fileParallelism: true,
    reporters: ['default'],
    coverage: {
      provider: 'v8',
    },
    projects: [
      {
        extends: true,
        test: {
          name: 'unit',
          environment: 'node',
          include: [
            'lib/__tests__/**/*.test.{ts,tsx}',
            'hooks/__tests__/**/*.test.{ts,tsx}',
          ],
          exclude: [
            'e2e/**',
            'node_modules/**',
            'lib/__tests__/api-schema.test.ts',
          ],
        },
      },
      {
        extends: true,
        test: {
          name: 'integration',
          environment: 'node',
          include: [
            'lib/__tests__/api-schema.test.ts',
            'lib/__tests__/**/*.integration.test.{ts,tsx}',
          ],
        },
      },
      // Component layer — Vitest Browser Mode. Enable by installing
      // @vitest/browser + playwright and adding a glob under include.
      // Kept inactive until Phase 3 of the adoption plan.
    ],
  },
})
