'use client'

import { useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { hasMenu, isLoggedIn } from '@/lib/auth'

/**
 * Redirect to dashboard if the current user does not have access to the given menu.
 * No-op for users with the `super` role (all menus accessible).
 *
 * Usage: `usePageGuard('audit')` at the top of a page component.
 */
export function usePageGuard(menuId: string): void {
  const router = useRouter()
  useEffect(() => {
    if (!isLoggedIn()) return // auth-guard handles login redirect
    if (!hasMenu(menuId)) {
      router.replace('/overview')
    }
  }, [menuId, router])
}
