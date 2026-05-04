'use client'

import { useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { hasPermission, isLoggedIn } from '@/lib/auth'
import type { Permission } from '@/lib/generated/Permission'

/**
 * Redirect to dashboard when the current user lacks the permission required
 * by this page. The permission must match the `Require*` extractor used by
 * the page's API endpoints (see `web/lib/route-permissions.ts`). Super-role
 * accounts bypass the check.
 *
 * Usage: `usePageGuard('audit_view')` at the top of a page component.
 */
export function usePageGuard(permission: Permission): void {
  const router = useRouter()
  useEffect(() => {
    if (!isLoggedIn()) return // auth-guard handles login redirect
    if (!hasPermission(permission)) {
      router.replace('/overview')
    }
  }, [permission, router])
}
