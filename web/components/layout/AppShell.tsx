'use client'

import { Menu } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { cn } from '@/lib/utils'

interface AppShellProps {
  mobileBrand: React.ReactNode
  mobileTopbarRight?: React.ReactNode
  mobileOpen: boolean
  onMobileToggle: () => void
  onMobileClose: () => void
  /** Whether the desktop sidebar is collapsed (icons-only). */
  collapsed?: boolean
  sidebar: React.ReactNode
  desktopHeader?: React.ReactNode
  children: React.ReactNode
}

export function AppShell({
  mobileBrand,
  mobileTopbarRight,
  mobileOpen,
  onMobileToggle,
  onMobileClose,
  collapsed = false,
  sidebar,
  desktopHeader,
  children,
}: AppShellProps) {
  const { t } = useTranslation()

  return (
    <div className="vds-flex vds-bg-page" style={{ height: '100dvh' }}>
      {/* ── Mobile top bar ───────────────────────────────────────────── */}
      <div
        className={cn(
          'vds-md:hidden vds-fixed vds-top-0 vds-left-0 vds-right-0 vds-z-sticky',
          'vds-flex vds-items-center vds-justify-between vds-h-12 vds-px-4',
          'vds-bg-card vds-border-b-1 vds-border-subtle vds-flex-shrink-0',
        )}
      >
        <div className="vds-flex vds-items-center vds-gap-3">
          <button
            type="button"
            onClick={onMobileToggle}
            className="vds-p-2 vds-rounded-md vds-text-dim vds-hover:text-primary vds-hover:bg-hover vds-transition-colors"
            aria-label={t('common.menu')}
            aria-expanded={mobileOpen}
            aria-controls="primary-nav"
          >
            <Menu className="vds-h-5 vds-w-5" aria-hidden />
          </button>
          {mobileBrand}
        </div>
        {mobileTopbarRight}
      </div>

      {/* ── Backdrop ─────────────────────────────────────────────────── */}
      {mobileOpen && (
        <div
          className="vds-md:hidden vds-fixed vds-inset-0 vds-z-overlay vds-bg-primary/30"
          onClick={onMobileClose}
          aria-hidden="true"
        />
      )}

      {/* ── Sidebar (responsive: overlay on mobile, static on desktop) ─
       * NOTE: do NOT use inline `style` for transform/width — inline beats
       * responsive `vds-md:*` classes by specificity and traps the sidebar
       * offscreen on desktop. Use the `veronex-aside` helper classes from
       * globals.css instead, which collapse the transform on desktop. */}
      <aside
        id="primary-nav"
        className={cn(
          'vds-flex vds-flex-col vds-bg-card vds-border-r-1 vds-border-subtle',
          'vds-fixed vds-inset-y-0 vds-left-0 vds-z-modal vds-max-w-72',
          'vds-transition-all vds-duration-medium vds-ease-ease-in-out',
          'vds-md:static vds-md:z-base vds-md:flex-shrink-0',
          collapsed ? 'vds-md:w-16' : 'vds-md:w-64',
          mobileOpen ? 'veronex-aside-open' : 'veronex-aside-closed',
        )}
      >
        {sidebar}
      </aside>

      {/* ── Main ─────────────────────────────────────────────────────── */}
      <div className="vds-flex-1 vds-overflow-auto vds-overscroll-contain">
        {desktopHeader && (
          <div className="vds-hidden vds-md:block">{desktopHeader}</div>
        )}
        <div className="vds-p-4 vds-pt-16 vds-md:p-8 vds-md:pt-8">
          {children}
        </div>
      </div>
    </div>
  )
}
