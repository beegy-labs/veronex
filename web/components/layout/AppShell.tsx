'use client'

import { Menu } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { cn } from '@/lib/utils'

interface AppShellProps {
  /** Mobile topbar brand slot */
  mobileBrand: React.ReactNode
  /** Mobile topbar right slot */
  mobileTopbarRight?: React.ReactNode
  /** Whether mobile drawer is open */
  mobileOpen: boolean
  /** Toggle mobile drawer */
  onMobileToggle: () => void
  /** Close mobile drawer */
  onMobileClose: () => void
  /**
   * Desktop sidebar width — pass a responsive Tailwind class with md: prefix,
   * e.g. 'md:w-56' or 'md:w-14'. Applied alongside mobile responsive classes.
   */
  sidebarWidth?: string
  /** Sidebar content (use SidebarFrame) */
  sidebar: React.ReactNode
  /** Sticky header rendered below topbar on desktop (optional) */
  desktopHeader?: React.ReactNode
  /** Main content */
  children: React.ReactNode
}

export function AppShell({
  mobileBrand,
  mobileTopbarRight,
  mobileOpen,
  onMobileToggle,
  onMobileClose,
  sidebarWidth = 'md:w-56',
  sidebar,
  desktopHeader,
  children,
}: AppShellProps) {
  const { t } = useTranslation()

  return (
    <div className="flex h-[100dvh] bg-background">
      {/* ── Mobile top bar ───────────────────────────────────────────── */}
      <div className="md:hidden fixed top-0 left-0 right-0 z-30 flex items-center justify-between h-12 px-4 bg-card border-b border-border flex-shrink-0">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onMobileToggle}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
            aria-label={t('common.menu')}
          >
            <Menu className="h-5 w-5" />
          </button>
          {mobileBrand}
        </div>
        {mobileTopbarRight}
      </div>

      {/* ── Backdrop ─────────────────────────────────────────────────── */}
      {mobileOpen && (
        <div
          className="md:hidden fixed inset-0 z-40 bg-foreground/30"
          onClick={onMobileClose}
          aria-hidden="true"
        />
      )}

      {/* ── Sidebar (responsive: overlay on mobile, static on desktop) ─ */}
      <aside
        className={cn(
          'flex flex-col bg-card border-r border-border',
          'fixed inset-y-0 left-0 z-50 w-[80vw] max-w-72',
          'transition-all duration-200 ease-in-out',
          mobileOpen ? 'translate-x-0' : '-translate-x-full',
          'md:static md:z-auto md:translate-x-0 md:flex-shrink-0',
          sidebarWidth,
        )}
      >
        {sidebar}
      </aside>

      {/* ── Main ─────────────────────────────────────────────────────── */}
      <main className="flex-1 overflow-auto">
        {desktopHeader && (
          <div className="hidden md:block">{desktopHeader}</div>
        )}
        <div className="p-4 pt-16 md:p-8 md:pt-8">
          {children}
        </div>
      </main>
    </div>
  )
}
