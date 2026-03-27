/** E2E test constants — single source of truth for URLs and credentials. */

/** Backend API base URL for direct API tests. */
export const API_BASE_URL =
  process.env.PLAYWRIGHT_API_URL ?? 'http://localhost:3001'

/** Default test credentials. */
export const TEST_USERNAME = process.env.E2E_USERNAME ?? 'test'
export const TEST_PASSWORD = process.env.E2E_PASSWORD ?? 'test1234!'

/** Generate a short unique suffix for test resource names. */
export function testId(): string {
  return crypto.randomUUID().slice(0, 8)
}

// ── E2E timeouts ────────────────────────────────────────────────────────────

/** Default visibility timeout for most UI assertions. */
export const T_DEFAULT = 10_000

/** Short timeout for quick assertions (toast, redirect). */
export const T_SHORT = 5_000

/** Long timeout for slow pages (audit log). */
export const T_LONG = 15_000
