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
        {!collapsed && <div className="flex-1 min-w-0">{brand}</div>}
        <button
          type="button"
          onClick={onToggle}
          className={cn(
            'p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0',
            'hidden md:block',
            collapsed && 'mx-auto',
          )}
          title={collapsed ? t('common.expand') : t('common.collapse')}
          aria-label={collapsed ? t('common.expand') : t('common.collapse')}
        >
          {collapsed
            ? <ChevronRight className="h-4 w-4" />
            : <ChevronLeft className="h-4 w-4" />
          }
        </button>
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
