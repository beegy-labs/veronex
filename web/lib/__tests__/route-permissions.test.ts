import { describe, it, expect } from 'vitest'
import { ROUTE_PERMISSION, type AdminRoute } from '../route-permissions'
import type { Permission } from '../generated/Permission'

// SSOT contract: every permission used by ROUTE_PERMISSION must be a real
// `Permission` known to the backend (the `Permission` type is generated from
// the Rust enum). A typo here would compile but silently lock users out, so
// we pin the union explicitly and let TS catch drift.
const KNOWN_PERMISSIONS: Permission[] = [
  'dashboard_view',
  'api_test',
  'provider_manage',
  'key_manage',
  'account_manage',
  'audit_view',
  'settings_manage',
  'role_manage',
  'model_manage',
  'mcp_manage',
]

describe('ROUTE_PERMISSION', () => {
  it('maps every admin route to a known permission', () => {
    for (const [route, perm] of Object.entries(ROUTE_PERMISSION)) {
      expect(KNOWN_PERMISSIONS).toContain(perm satisfies Permission)
      expect(route satisfies AdminRoute).toBeTruthy()
    }
  })

  it('keeps /mcp on a dedicated mcp_manage permission (regression: was provider_manage)', () => {
    expect(ROUTE_PERMISSION['/mcp']).toBe('mcp_manage')
  })

  it('keeps every read-only dashboard page on dashboard_view', () => {
    for (const r of ['/overview', '/usage', '/performance', '/health', '/flow', '/jobs'] as const) {
      expect(ROUTE_PERMISSION[r]).toBe('dashboard_view')
    }
  })
})
