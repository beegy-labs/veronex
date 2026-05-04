// SSOT — route ↔ permission map.
//
// Each admin page gates on a single permission. Both the sidebar item
// (`nav.tsx`) and the page-level guard (`usePageGuard`) read this map, and the
// permission must match the strictest `Require*` extractor on the route's
// underlying API endpoints — otherwise users land on a page whose data they
// cannot fetch (404/403 stale-cache surface).
//
// When adding a new admin page, add an entry here AND verify the
// corresponding handler in `crates/veronex/src/infrastructure/inbound/http/`
// uses the matching `Require<Permission>` extractor.

import type { Permission } from '@/lib/generated/Permission'

export type AdminRoute =
  | '/overview'
  | '/usage'
  | '/performance'
  | '/health'
  | '/flow'
  | '/jobs'
  | '/test'
  | '/keys'
  | '/servers'
  | '/providers'
  | '/mcp'
  | '/accounts'
  | '/audit'
  | '/api-docs'

export const ROUTE_PERMISSION: Record<AdminRoute, Permission> = {
  '/overview':    'dashboard_view',
  '/usage':       'dashboard_view',
  '/performance': 'dashboard_view',
  '/health':      'dashboard_view',
  '/flow':        'dashboard_view',
  '/jobs':        'dashboard_view',
  '/test':        'api_test',
  '/keys':        'key_manage',
  '/servers':     'provider_manage',
  '/providers':   'provider_manage',
  '/mcp':         'mcp_manage',
  '/accounts':    'account_manage',
  '/audit':       'audit_view',
  '/api-docs':    'dashboard_view',
}
