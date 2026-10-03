import { describe, it, expect } from 'vitest'
import { ROUTE_PERMISSION } from '../route-permissions'
describe('ROUTE_PERMISSION', () => {
  it('keeps /mcp on a dedicated mcp_manage permission (regression: was provider_manage)', () => {
    expect(ROUTE_PERMISSION['/mcp']).toBe('mcp_manage')
  })

  it('keeps every read-only dashboard page on dashboard_view', () => {
    for (const r of ['/overview', '/usage', '/performance', '/health', '/flow', '/jobs'] as const) {
      expect(ROUTE_PERMISSION[r]).toBe('dashboard_view')
    }
  })
})
