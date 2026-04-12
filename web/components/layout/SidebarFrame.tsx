'use client'

import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { cn } from '@/lib/utils'

interface SidebarFrameProps {
  /** Whether sidebar is in collapsed (icon-only) mode — desktop only */
  collapsed: boolean
  /** Toggle collapse */
  onToggle: () => void
  /** Brand slot: rendered when sidebar is expanded */
  brand: React.ReactNode
  /** Icon slot: rendered when sidebar is collapsed (click expands) */
  icon?: React.ReactNode
  /** Optional context switcher rendered below the brand */
  contextSwitcher?: React.ReactNode
  /** Main scrollable nav area */
  nav: React.ReactNode
  /** Bottom section: theme toggle, user info, etc. */
  bottom?: React.ReactNode
}

export function SidebarFrame({
  collapsed,
  onToggle,
  brand,
  icon,
  contextSwitcher,
  nav,
  bottom,
}: SidebarFrameProps) {
  const { t } = useTranslation()

  return (
    <>
      {/* ── Brand + collapse toggle ─────────────────────────────────── */}
      <div className={cn(
        'h-[60px] flex items-center border-b border-border flex-shrink-0',
        collapsed ? 'justify-center px-0' : 'px-4 gap-2',
      )}>
        {collapsed ? (
          /* Collapsed: logo icon acts as expand button */
          <button
            type="button"
            onClick={onToggle}
            className="flex items-center justify-center p-1 rounded-md hover:bg-accent transition-colors hidden md:flex"
            title={t('common.expand')}
            aria-label={t('common.expand')}
          >
            {icon}
          </button>
        ) : (
          /* Expanded: full brand + collapse chevron */
          <>
            <div className="flex-1 min-w-0">{brand}</div>
            <button
              type="button"
              onClick={onToggle}
              className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0 hidden md:block"
              title={t('common.collapse')}
              aria-label={t('common.collapse')}
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
          </>
        )}
      </div>

      {/* ── Context switcher (optional) ─────────────────────────────── */}
      {contextSwitcher && (
        <div className={cn(
          'border-b border-border flex-shrink-0',
          collapsed ? 'px-1.5 py-1.5' : 'px-2 pt-2 pb-1.5',
        )}>
          {contextSwitcher}
        </div>
      )}

      {/* ── Nav ─────────────────────────────────────────────────────── */}
      <nav className="flex-1 py-3 px-2 overflow-y-auto">
        {nav}
      </nav>

      {/* ── Bottom ──────────────────────────────────────────────────── */}
      {bottom && (
        <div className="border-t border-border flex-shrink-0">
          {bottom}
        </div>
      )}
    </>
  )
}
