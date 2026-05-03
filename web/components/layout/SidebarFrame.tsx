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
        'vds-h-[60px] vds-flex vds-items-center vds-border-b-1 vds-border-subtle vds-flex-shrink-0',
        collapsed ? 'vds-justify-center vds-px-0' : 'vds-px-4 vds-gap-2',
      )}>
        {collapsed ? (
          /* Collapsed: logo icon acts as expand button */
          <button
            type="button"
            onClick={onToggle}
            className="vds-flex vds-items-center vds-justify-center vds-p-1 vds-rounded-md vds-hover:bg-hover vds-transition-colors vds-hidden vds-md:flex"
            title={t('common.expand')}
            aria-label={t('common.expand')}
          >
            {icon}
          </button>
        ) : (
          /* Expanded: full brand + collapse chevron */
          <>
            <div className="vds-flex-1 vds-min-w-0">{brand}</div>
            <button
              type="button"
              onClick={onToggle}
              className="vds-p-1.5 vds-rounded-md vds-text-dim vds-hover:text-primary vds-hover:bg-hover vds-transition-colors vds-flex-shrink-0 vds-hidden vds-md:block"
              title={t('common.collapse')}
              aria-label={t('common.collapse')}
            >
              <ChevronLeft className="vds-h-4 vds-w-4" />
            </button>
          </>
        )}
      </div>

      {/* ── Context switcher (optional) ─────────────────────────────── */}
      {contextSwitcher && (
        <div className={cn(
          'vds-border-b-1 vds-border-subtle vds-flex-shrink-0',
          collapsed ? 'vds-px-1.5 vds-py-1.5' : 'vds-px-2 vds-pt-2 vds-pb-1.5',
        )}>
          {contextSwitcher}
        </div>
      )}

      {/* ── Nav ─────────────────────────────────────────────────────── */}
      <nav className="vds-flex-1 vds-py-3 vds-px-2 vds-overflow-y-auto">
        {nav}
      </nav>

      {/* ── Bottom ──────────────────────────────────────────────────── */}
      {bottom && (
        <div className="vds-border-t-1 vds-border-subtle vds-flex-shrink-0">
          {bottom}
        </div>
      )}
    </>
  )
}
