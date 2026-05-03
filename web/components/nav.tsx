'use client'

import Link from 'next/link'
import { usePathname, useSearchParams } from 'next/navigation'
import { useState, useEffect, Suspense } from 'react'
import {
  LayoutDashboard, List, Key, Server, Activity,
  BarChart2, Gauge, Sun, Moon,
  BookOpen, HardDrive, Sparkles, ChevronDown,
  Users, Shield, LogOut, Settings2, Plug,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { useTheme } from '@/components/theme-provider'
import { useTranslation } from '@/i18n'
import { getAuthUser, hasPermission } from '@/lib/auth'
import type { Permission } from '@/lib/generated/Permission'
import { redirectToLogin } from '@/lib/auth-guard'
import { useLabSettings } from '@/components/lab-settings-provider'
import { useTimezone } from '@/components/timezone-provider'
import { NavSettingsDialog } from '@/components/nav-settings-dialog'
import { HexLogo, OllamaIcon } from '@/components/nav-icons'
import { useNav404 } from '@/components/nav-404-context'
import { SidebarFrame } from '@/components/layout/SidebarFrame'

// ── Constants ──────────────────────────────────────────────────────────────────

const groupStorageKey = (id: string) => `nav-group-${id}`

// ── Nav item types ──────────────────────────────────────────────────────────────

type NavLink = {
  type: 'link'
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  /** Permission required to view the page. Same as the strictest `Require*`
   *  extractor on the page's API endpoints — see `lib/route-permissions.ts`. */
  permission: Permission
  section?: string
}

type NavGroupChild = {
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  section?: string
  permission: Permission
}

type NavGroup = {
  type: 'group'
  id: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  basePath: string
  children: NavGroupChild[]
  /** Group is visible if its permission is granted. Children may have
   *  stricter permissions and are filtered independently. */
  permission: Permission
}

type NavItem = NavLink | NavGroup

// ── Nav structure ───────────────────────────────────────────────────────────────

const navItems: NavItem[] = [
  {
    type: 'group',
    id: 'overview',
    labelKey: 'nav.monitor',
    icon: LayoutDashboard,
    basePath: '/overview',
    permission: 'dashboard_view',
    children: [
      { href: '/overview',     labelKey: 'nav.dashboard',   icon: LayoutDashboard, permission: 'dashboard_view' },
      { href: '/usage',        labelKey: 'nav.usage',       icon: BarChart2,       permission: 'dashboard_view' },
      { href: '/performance',  labelKey: 'nav.performance', icon: Gauge,           permission: 'dashboard_view' },
    ],
  },
  { type: 'link', href: '/health',  labelKey: 'nav.health',  icon: Activity,  permission: 'dashboard_view' },
  { type: 'link', href: '/jobs',    labelKey: 'nav.jobs',    icon: List,      permission: 'dashboard_view' },
  { type: 'link', href: '/keys',    labelKey: 'nav.keys',    icon: Key,       permission: 'key_manage' },
  { type: 'link', href: '/servers', labelKey: 'nav.servers', icon: HardDrive, permission: 'provider_manage' },
  {
    type: 'group',
    id: 'providers',
    labelKey: 'nav.providers',
    icon: Server,
    basePath: '/providers',
    permission: 'provider_manage',
    children: [
      { href: '/providers?s=ollama', labelKey: 'nav.ollama', icon: OllamaIcon, section: 'ollama', permission: 'provider_manage' },
      { href: '/providers?s=gemini', labelKey: 'nav.gemini', icon: Sparkles,   section: 'gemini', permission: 'provider_manage' },
    ],
  },
  { type: 'link', href: '/mcp', labelKey: 'nav.mcp', icon: Plug, permission: 'mcp_manage', section: 'mcp' },
]

// ── Nav props ───────────────────────────────────────────────────────────────────

interface NavContentProps {
  collapsed: boolean
  onToggle: () => void
}

// ── Inner nav (needs useSearchParams — wrapped in Suspense by parent) ───────────

function NavContent({ collapsed, onToggle }: NavContentProps) {
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const { theme, toggleTheme } = useTheme()
  const { t } = useTranslation()
  const { resetToLocaleDefault } = useTimezone()
  const { hidden: nav404 } = useNav404()

  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({})
  const [authUser, setAuthUser] = useState<{ username: string; role: string } | null>(null)
  const { labSettings } = useLabSettings()
  const [showSettings, setShowSettings] = useState(false)

  // Restore persisted state on mount
  useEffect(() => {
    setAuthUser(getAuthUser())

    const groups: Record<string, boolean> = {}
    for (const item of navItems) {
      if (item.type === 'group') {
        const saved = localStorage.getItem(groupStorageKey(item.id))
        const defaultOpen = item.id === 'overview'
        groups[item.id] = saved !== null ? saved === 'true' : defaultOpen
      }
    }
    setOpenGroups(groups)
  }, [])

  // Auto-open the group containing the active route
  useEffect(() => {
    for (const item of navItems) {
      if (item.type !== 'group') continue
      const isActive = item.children.some((child) =>
        child.section
          ? pathname === item.basePath && (searchParams.get('s') ?? 'ollama') === child.section
          : pathname === child.href,
      )
      if (isActive) {
        setOpenGroups((prev) => {
          if (prev[item.id]) return prev
          const next = { ...prev, [item.id]: true }
          localStorage.setItem(groupStorageKey(item.id), 'true')
          return next
        })
      }
    }
  }, [pathname, searchParams])

  function expandAndOpenGroup(id: string) {
    if (collapsed) onToggle()
    setOpenGroups((prev) => {
      const next = { ...prev, [id]: true }
      localStorage.setItem(groupStorageKey(id), 'true')
      return next
    })
  }

  function toggleGroup(id: string) {
    setOpenGroups((prev) => {
      const next = { ...prev, [id]: !prev[id] }
      localStorage.setItem(groupStorageKey(id), String(next[id]))
      return next
    })
  }

  function isChildActive(child: NavGroupChild, basePath: string): boolean {
    if (child.section) {
      if (pathname !== basePath) return false
      return (searchParams.get('s') ?? 'ollama') === child.section
    }
    return pathname === child.href
  }

  function isGroupActive(item: NavGroup): boolean {
    return item.children.some((child) => isChildActive(child, item.basePath))
  }

  // ── Nav items (filtered + rendered) ──────────────────────────────────────────

  const visibleItems = navItems
    .filter(item => hasPermission(item.permission))
    .filter(item => !('section' in item) || !item.section || !nav404.has(item.section))
    .map(item => {
      if (item.type === 'group') {
        return {
          ...item,
          children: item.children
            .filter(c => hasPermission(c.permission))
            .filter(c => !c.section || !nav404.has(c.section))
            .filter(c =>
              item.id !== 'providers' || c.section !== 'gemini' || (labSettings?.gemini_function_calling ?? false)
            ),
        }
      }
      return item
    })
    .filter(item => item.type !== 'group' || item.children.length > 0)

  const navLinks = (
    <div className="vds-space-y-0.5">
      {visibleItems.map((item) => {
        if (item.type === 'link') {
          const active = pathname.startsWith(item.href)
          return (
            <Link
              key={item.href}
              href={item.href}
              title={collapsed ? t(item.labelKey) : undefined}
              className={cn(
                'vds-flex vds-items-center vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
                collapsed ? 'vds-justify-center vds-h-9 vds-w-9 vds-mx-auto' : 'vds-gap-3 vds-px-3 vds-py-2',
                active
                  ? 'vds-bg-primary vds-text-primary-fg'
                  : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
              )}
            >
              <item.icon className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
              {!collapsed && t(item.labelKey)}
            </Link>
          )
        }

        // ── Group ───────────────────────────────────────────────────────────
        const groupActive = isGroupActive(item)
        const groupOpen = openGroups[item.id] ?? false

        return (
          <div key={item.id}>
            {collapsed ? (
              <button
                type="button"
                title={t(item.labelKey)}
                onClick={() => expandAndOpenGroup(item.id)}
                className={cn(
                  'vds-flex vds-items-center vds-justify-center vds-h-9 vds-w-9 vds-mx-auto vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
                  groupActive
                    ? 'vds-bg-primary/15 vds-text-primary'
                    : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
                )}
              >
                <item.icon className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
              </button>
            ) : (
              <button
                type="button"
                onClick={() => toggleGroup(item.id)}
                className={cn(
                  'vds-w-full vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2 vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
                  groupActive
                    ? 'vds-text-primary'
                    : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
                )}
              >
                <item.icon className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
                <span className="vds-flex-1 vds-text-left">{t(item.labelKey)}</span>
                <ChevronDown
                  className={cn(
                    'vds-h-3.5 vds-w-3.5 vds-flex-shrink-0 vds-transition-transform vds-duration-medium',
                    groupOpen && 'vds-rotate-180',
                  )}
                />
              </button>
            )}

            {!collapsed && groupOpen && (
              <div className="vds-mt-0.5 vds-ml-3 vds-pl-3 vds-border-l-1 vds-border-subtle vds-space-y-0.5">
                {item.children.map((child) => {
                  const active = isChildActive(child, item.basePath)
                  return (
                    <Link
                      key={child.href}
                      href={child.href}
                      className={cn(
                        'vds-flex vds-items-center vds-gap-2.5 vds-px-2 vds-py-1.5 vds-rounded-md vds-text-sm vds-transition-colors',
                        active
                          ? 'vds-bg-primary vds-text-primary-fg vds-font-500'
                          : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
                      )}
                    >
                      <child.icon className="vds-h-3.5 vds-w-3.5 vds-flex-shrink-0" />
                      {t(child.labelKey)}
                    </Link>
                  )
                })}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )

  // ── Footer slots ──────────────────────────────────────────────────────────────

  const bottomSlot = (
    <div className="vds-py-3 vds-px-2 vds-space-y-2">
      {authUser && !collapsed && (
        <div className="vds-px-1 vds-space-y-0.5">
          {hasPermission('account_manage') && (
            <Link
              href="/accounts"
              className={cn(
                'vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2 vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
                pathname.startsWith('/accounts')
                  ? 'vds-bg-primary vds-text-primary-fg'
                  : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
              )}
            >
              <Users className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
              {t('accounts.title')}
            </Link>
          )}
          {hasPermission('audit_view') && (
            <Link
              href="/audit"
              className={cn(
                'vds-flex vds-items-center vds-gap-3 vds-px-3 vds-py-2 vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
                pathname.startsWith('/audit')
                  ? 'vds-bg-primary vds-text-primary-fg'
                  : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
              )}
            >
              <Shield className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
              {t('audit.title')}
            </Link>
          )}
        </div>
      )}

      <div className="vds-px-1">
        <Link
          href="/api-docs"
          title={collapsed ? t('nav.apiDocs') : undefined}
          className={cn(
            'vds-flex vds-items-center vds-rounded-md vds-text-sm vds-font-500 vds-transition-colors',
            collapsed ? 'vds-justify-center vds-h-9 vds-w-9 vds-mx-auto' : 'vds-gap-3 vds-px-3 vds-py-2',
            pathname.startsWith('/api-docs')
              ? 'vds-bg-primary vds-text-primary-fg'
              : 'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
          )}
        >
          <BookOpen className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
          {!collapsed && t('nav.apiDocs')}
        </Link>
      </div>

      {authUser && !collapsed && (
        <div className="vds-px-1">
          <div className="vds-flex vds-items-center vds-justify-between vds-px-3 vds-py-1.5 vds-rounded-md vds-hover:bg-hover/50 vds-transition-colors">
            <span className="vds-text-xs vds-text-dim vds-truncate">{authUser.username}</span>
            <button
              type="button"
              aria-label={t('common.signOut')}
              title={t('common.signOut')}
              onClick={() => redirectToLogin()}
              className="vds-p-1 vds-rounded-md vds-text-dim vds-hover:text-primary vds-hover:bg-hover vds-transition-colors"
            >
              <LogOut className="vds-h-3.5 vds-w-3.5" />
            </button>
          </div>
        </div>
      )}

      <div className={cn(
        'vds-flex vds-items-center vds-gap-1 vds-px-1',
        collapsed ? 'vds-justify-center vds-flex-col vds-gap-0.5' : 'vds-justify-between',
      )}>
        {!collapsed && <p className="vds-text-xs vds-text-dim vds-flex-shrink-0">v0.1.0</p>}

        <button
          type="button"
          onClick={() => setShowSettings(true)}
          className="vds-p-1.5 vds-rounded-md vds-text-dim vds-hover:text-primary vds-hover:bg-hover vds-transition-colors vds-flex-shrink-0"
          aria-label={t('common.settings')}
          title={t('common.settings')}
        >
          <Settings2 className="vds-h-4 vds-w-4" />
        </button>

        <button
          type="button"
          onClick={toggleTheme}
          className="vds-p-1.5 vds-rounded-md vds-text-dim vds-hover:text-primary vds-hover:bg-hover vds-transition-colors vds-flex-shrink-0"
          aria-label={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
          title={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
        >
          {theme === 'dark' ? <Sun className="vds-h-4 vds-w-4" /> : <Moon className="vds-h-4 vds-w-4" />}
        </button>
      </div>

      <NavSettingsDialog
        open={showSettings}
        onClose={() => setShowSettings(false)}
        resetToLocaleDefault={resetToLocaleDefault}
      />
    </div>
  )

  return (
    <SidebarFrame
      collapsed={collapsed}
      onToggle={onToggle}
      icon={<HexLogo className="vds-h-7 vds-w-7" />}
      brand={
        <div className="vds-flex vds-items-center vds-gap-2.5">
          <HexLogo className="vds-h-7 vds-w-7 vds-flex-shrink-0" />
          <span className="vds-text-base vds-font-600 vds-tracking-tight vds-truncate">Veronex</span>
        </div>
      }
      nav={navLinks}
      bottom={bottomSlot}
    />
  )
}

// ── Nav props (public) ──────────────────────────────────────────────────────────

interface NavProps {
  collapsed: boolean
  onToggle: () => void
}

// ── Nav (Suspense wrapper for useSearchParams) ──────────────────────────────────

export default function Nav({ collapsed, onToggle }: NavProps) {
  return (
    <Suspense fallback={null}>
      <NavContent collapsed={collapsed} onToggle={onToggle} />
    </Suspense>
  )
}
