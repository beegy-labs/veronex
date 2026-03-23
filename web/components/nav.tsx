'use client'

import Link from 'next/link'
import { usePathname, useSearchParams } from 'next/navigation'
import { useState, useEffect, Suspense } from 'react'
import {
  LayoutDashboard, List, Key, Server,
  BarChart2, Gauge, Sun, Moon, ChevronLeft,
  BookOpen, HardDrive, Sparkles, ChevronDown, Menu,
  Users, Shield, LogOut, Settings2, Plug,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { useTheme } from '@/components/theme-provider'
import { useTranslation } from '@/i18n'
import { getAuthUser, hasMenu } from '@/lib/auth'
import { redirectToLogin } from '@/lib/auth-guard'
import { useLabSettings } from '@/components/lab-settings-provider'
import { useTimezone } from '@/components/timezone-provider'
import { NavSettingsDialog } from '@/components/nav-settings-dialog'
import { HexLogo, OllamaIcon } from '@/components/nav-icons'

// ── Constants ──────────────────────────────────────────────────────────────────

const NAV_COLLAPSED_KEY = 'nav-collapsed'
const groupStorageKey = (id: string) => `nav-group-${id}`

// ── Nav item types ──────────────────────────────────────────────────────────────

type NavLink = {
  type: 'link'
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  /** Menu ID for role-based visibility filtering. */
  menuId?: string
}

type NavGroupChild = {
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  section?: string  // if set: matched via ?s= query param; otherwise: pathname === href
  /** Menu ID for role-based visibility filtering. */
  menuId?: string
}

type NavGroup = {
  type: 'group'
  id: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  basePath: string
  children: NavGroupChild[]
  /** Menu ID for role-based visibility filtering (applies to entire group). */
  menuId?: string
}

type NavItem = NavLink | NavGroup

// ── Nav structure ───────────────────────────────────────────────────────────────
// Add new providers here — sub-items appear automatically in the sidebar.

const navItems: NavItem[] = [
  {
    type: 'group',
    id: 'overview',
    labelKey: 'nav.monitor',
    icon: LayoutDashboard,
    basePath: '/overview',
    menuId: 'dashboard',
    children: [
      { href: '/overview',     labelKey: 'nav.dashboard',   icon: LayoutDashboard, menuId: 'dashboard' },
      { href: '/usage',        labelKey: 'nav.usage',       icon: BarChart2,       menuId: 'usage' },
      { href: '/performance',  labelKey: 'nav.performance', icon: Gauge,           menuId: 'performance' },
    ],
  },
  { type: 'link', href: '/jobs',    labelKey: 'nav.jobs',    icon: List,      menuId: 'jobs' },
  { type: 'link', href: '/keys',    labelKey: 'nav.keys',    icon: Key,       menuId: 'keys' },
  { type: 'link', href: '/servers', labelKey: 'nav.servers', icon: HardDrive, menuId: 'servers' },
  {
    type: 'group',
    id: 'providers',
    labelKey: 'nav.providers',
    icon: Server,
    basePath: '/providers',
    menuId: 'providers',
    children: [
      { href: '/providers?s=ollama', labelKey: 'nav.ollama', icon: OllamaIcon, section: 'ollama', menuId: 'providers' },
      { href: '/providers?s=gemini', labelKey: 'nav.gemini', icon: Sparkles,   section: 'gemini', menuId: 'providers' },
      { href: '/providers?s=mcp',    labelKey: 'nav.mcp',    icon: Plug,       section: 'mcp',    menuId: 'providers' },
    ],
  },
]

// ── Inner nav (needs useSearchParams — wrapped in Suspense by parent) ───────────

function NavContent() {
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const { theme, toggleTheme } = useTheme()
  const { t } = useTranslation()
  const { resetToLocaleDefault } = useTimezone()

  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({})
  const [authUser, setAuthUser] = useState<{ username: string; role: string } | null>(null)
  const { labSettings } = useLabSettings()
  const [showSettings, setShowSettings] = useState(false)

  // Restore persisted state on mount
  useEffect(() => {
    const user = getAuthUser()
    setAuthUser(user)

    const savedCollapsed = localStorage.getItem(NAV_COLLAPSED_KEY)
    if (savedCollapsed === 'true') setCollapsed(true)

    const groups: Record<string, boolean> = {}
    for (const item of navItems) {
      if (item.type === 'group') {
        const saved = localStorage.getItem(groupStorageKey(item.id))
        // overview group defaults to open; others default to closed
        const defaultOpen = item.id === 'overview'
        groups[item.id] = saved !== null ? saved === 'true' : defaultOpen
      }
    }
    setOpenGroups(groups)
  }, [])

  // Close mobile nav on route change
  useEffect(() => { setMobileOpen(false) }, [pathname])

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

  function toggleCollapsed() {
    setCollapsed((v) => {
      const next = !v
      localStorage.setItem(NAV_COLLAPSED_KEY, String(next))
      return next
    })
  }

  function expandAndOpenGroup(id: string) {
    setCollapsed(false)
    localStorage.setItem(NAV_COLLAPSED_KEY, 'false')
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

  return (
    <>
      {/* ── Mobile top bar ─────────────────────────────────────────── */}
      <div className="md:hidden fixed top-0 left-0 right-0 z-30 flex items-center h-12 px-4 bg-card border-b border-border gap-3 flex-shrink-0">
        <button
          type="button"
          onClick={() => setMobileOpen((v) => !v)}
          aria-label={t('common.menu')}
          className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          title={t('common.menu')}
        >
          <Menu className="h-5 w-5" />
        </button>
        <HexLogo className="h-6 w-6 flex-shrink-0" />
        <span className="text-sm font-semibold tracking-tight">Veronex</span>
      </div>

      {/* ── Backdrop ───────────────────────────────────────────────── */}
      {mobileOpen && (
        <div
          className="md:hidden fixed inset-0 z-40 bg-foreground/30"
          onClick={() => setMobileOpen(false)}
          aria-hidden="true"
        />
      )}

      {/* ── Sidebar ────────────────────────────────────────────────── */}
      <aside
        className={cn(
          // Base: always flex column, themed
          'flex flex-col bg-card border-r border-border',
          // Mobile: fixed overlay, slides in/out from left
          'fixed inset-y-0 left-0 z-50 w-[80vw] max-w-72',
          'transition-transform duration-200 ease-in-out',
          mobileOpen ? 'translate-x-0' : '-translate-x-full',
          // Desktop: back to normal flex child, collapsible width
          'md:static md:z-auto md:translate-x-0 md:flex-shrink-0',
          collapsed ? 'md:w-14' : 'md:w-56',
        )}
      >
      {/* ── Header ─────────────────────────────────────────────────── */}
      <div className={cn(
        'flex items-center border-b border-border h-[60px] flex-shrink-0',
        collapsed ? 'justify-center px-0' : 'px-4 gap-2.5',
      )}>
        {collapsed ? (
          <button
            type="button"
            onClick={toggleCollapsed}
            className="flex items-center justify-center"
            aria-label={t('common.expand')}
            title={t('common.expand')}
          >
            <HexLogo className="h-7 w-7" />
          </button>
        ) : (
          <>
            <HexLogo className="h-7 w-7 flex-shrink-0" />
            <span className="text-base font-semibold tracking-tight flex-1 truncate">Veronex</span>
            <button
              type="button"
              onClick={toggleCollapsed}
              className="p-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors flex-shrink-0"
              aria-label={t('common.collapse')}
              title={t('common.collapse')}
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
          </>
        )}
      </div>

      {/* ── Nav links ──────────────────────────────────────────────── */}
      <nav className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto">
        {navItems
          // Filter by role-based menu access
          .filter(item => !item.menuId || hasMenu(item.menuId))
          .map(item => {
            if (item.type === 'group') {
              return {
                ...item,
                children: item.children
                  .filter(c => !c.menuId || hasMenu(c.menuId))
                  .filter(c =>
                    item.id !== 'providers' || c.section !== 'gemini' || (labSettings?.gemini_function_calling ?? false)
                  ),
              }
            }
            return item
          })
          .filter(item => item.type !== 'group' || item.children.length > 0)
          .map((item) => {
          if (item.type === 'link') {
            const active = pathname.startsWith(item.href)
            return (
              <Link
                key={item.href}
                href={item.href}
                title={collapsed ? t(item.labelKey) : undefined}
                className={cn(
                  'flex items-center rounded-md text-sm font-medium transition-colors',
                  collapsed ? 'justify-center h-9 w-9 mx-auto' : 'gap-3 px-3 py-2',
                  active
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )}
              >
                <item.icon className="h-4 w-4 flex-shrink-0" />
                {!collapsed && t(item.labelKey)}
              </Link>
            )
          }

          // ── Group item ────────────────────────────────────────────
          const groupActive = isGroupActive(item)
          const groupOpen = openGroups[item.id] ?? false

          return (
            <div key={item.id}>
              {collapsed ? (
                /* Collapsed: single icon button → expand sidebar + open group */
                <button
                  type="button"
                  title={t(item.labelKey)}
                  onClick={() => expandAndOpenGroup(item.id)}
                  className={cn(
                    'flex items-center justify-center h-9 w-9 mx-auto rounded-md text-sm font-medium transition-colors',
                    groupActive
                      ? 'bg-primary/15 text-primary'
                      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                  )}
                >
                  <item.icon className="h-4 w-4 flex-shrink-0" />
                </button>
              ) : (
                /* Expanded: label + chevron toggle */
                <button
                  type="button"
                  onClick={() => toggleGroup(item.id)}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                    groupActive
                      ? 'text-primary'
                      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                  )}
                >
                  <item.icon className="h-4 w-4 flex-shrink-0" />
                  <span className="flex-1 text-left">{t(item.labelKey)}</span>
                  <ChevronDown
                    className={cn(
                      'h-3.5 w-3.5 shrink-0 transition-transform duration-150',
                      groupOpen && 'rotate-180',
                    )}
                  />
                </button>
              )}

              {/* Sub-items (visible only when sidebar expanded + group open) */}
              {!collapsed && groupOpen && (
                <div className="mt-0.5 ml-3 pl-3 border-l border-border space-y-0.5">
                  {item.children.map((child) => {
                    const active = isChildActive(child, item.basePath)
                    return (
                      <Link
                        key={child.href}
                        href={child.href}
                        className={cn(
                          'flex items-center gap-2.5 px-2 py-1.5 rounded-md text-sm transition-colors',
                          active
                            ? 'bg-primary text-primary-foreground font-medium'
                            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                        )}
                      >
                        <child.icon className="h-3.5 w-3.5 flex-shrink-0" />
                        {t(child.labelKey)}
                      </Link>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </nav>

      {/* ── Footer ─────────────────────────────────────────────────── */}
      <div className="border-t border-border py-3 px-2 space-y-2">
        {/* Auth user + JWT-protected links */}
        {authUser && !collapsed && (
          <div className="px-1 space-y-0.5">
            {hasMenu('accounts') && (
              <Link
                href="/accounts"
                className={cn(
                  'flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                  pathname.startsWith('/accounts')
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )}
              >
                <Users className="h-4 w-4 flex-shrink-0" />
                {t('accounts.title')}
              </Link>
            )}
            {hasMenu('audit') && (
              <Link
                href="/audit"
                className={cn(
                  'flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                  pathname.startsWith('/audit')
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )}
              >
                <Shield className="h-4 w-4 flex-shrink-0" />
                {t('audit.title')}
              </Link>
            )}
            <div className="flex items-center justify-between px-3 py-1">
              <span className="text-xs text-muted-foreground truncate">{authUser.username}</span>
              <button
                type="button"
                aria-label={t('common.signOut')}
                title={t('common.signOut')}
                onClick={() => redirectToLogin()}
                className="p-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
              >
                <LogOut className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        )}

        {/* API Docs — always visible */}
        <div className="px-1">
          <Link
            href="/api-docs"
            title={collapsed ? t('nav.apiDocs') : undefined}
            className={cn(
              'flex items-center rounded-md text-sm font-medium transition-colors',
              collapsed ? 'justify-center h-9 w-9 mx-auto' : 'gap-3 px-3 py-2',
              pathname.startsWith('/api-docs')
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
            )}
          >
            <BookOpen className="h-4 w-4 flex-shrink-0" />
            {!collapsed && t('nav.apiDocs')}
          </Link>
        </div>

        {/* Footer: version | settings gear | theme toggle */}
        <div className={cn(
          'flex items-center gap-1 px-1',
          collapsed ? 'justify-center flex-col gap-0.5' : 'justify-between',
        )}>
          {!collapsed && (
            <p className="text-xs text-muted-foreground shrink-0">v0.1.0</p>
          )}

          <button
            type="button"
            onClick={() => setShowSettings(true)}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
            aria-label={t('common.settings')}
            title={t('common.settings')}
          >
            <Settings2 className="h-4 w-4" />
          </button>

          <button
            type="button"
            onClick={toggleTheme}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
            aria-label={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
            title={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
          >
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>
        </div>

        <NavSettingsDialog
          open={showSettings}
          onClose={() => setShowSettings(false)}
          resetToLocaleDefault={resetToLocaleDefault}
        />
      </div>
    </aside>
    </>
  )
}

// ── Nav (Suspense wrapper for useSearchParams) ──────────────────────────────────

export default function Nav() {
  return (
    <Suspense fallback={<div className="flex-shrink-0 w-14 bg-card border-r border-border" />}>
      <NavContent />
    </Suspense>
  )
}
