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
import { getAuthUser, hasMenu } from '@/lib/auth'
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
  menuId?: string
  section?: string
}

type NavGroupChild = {
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  section?: string
  menuId?: string
}

type NavGroup = {
  type: 'group'
  id: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  basePath: string
  children: NavGroupChild[]
  menuId?: string
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
    menuId: 'dashboard',
    children: [
      { href: '/overview',     labelKey: 'nav.dashboard',   icon: LayoutDashboard, menuId: 'dashboard' },
      { href: '/usage',        labelKey: 'nav.usage',       icon: BarChart2,       menuId: 'usage' },
      { href: '/performance',  labelKey: 'nav.performance', icon: Gauge,           menuId: 'performance' },
    ],
  },
  { type: 'link', href: '/health',  labelKey: 'nav.health',  icon: Activity,  menuId: 'dashboard' },
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
    ],
  },
  { type: 'link', href: '/mcp', labelKey: 'nav.mcp', icon: Plug, menuId: 'providers', section: 'mcp' },
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
    .filter(item => !item.menuId || hasMenu(item.menuId))
    .filter(item => !('section' in item) || !item.section || !nav404.has(item.section))
    .map(item => {
      if (item.type === 'group') {
        return {
          ...item,
          children: item.children
            .filter(c => !c.menuId || hasMenu(c.menuId))
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
    <div className="space-y-0.5">
      {visibleItems.map((item) => {
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
                  'flex items-center justify-center h-9 w-9 mx-auto rounded-md text-sm font-medium transition-colors',
                  groupActive
                    ? 'bg-primary/15 text-primary'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )}
              >
                <item.icon className="h-4 w-4 flex-shrink-0" />
              </button>
            ) : (
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
    </div>
  )

  // ── Footer slots ──────────────────────────────────────────────────────────────

  const bottomSlot = (
    <div className="py-3 px-2 space-y-2">
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
        </div>
      )}

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

      {authUser && !collapsed && (
        <div className="px-1">
          <div className="flex items-center justify-between px-3 py-1.5 rounded-md hover:bg-accent/50 transition-colors">
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

      <div className={cn(
        'flex items-center gap-1 px-1',
        collapsed ? 'justify-center flex-col gap-0.5' : 'justify-between',
      )}>
        {!collapsed && <p className="text-xs text-muted-foreground shrink-0">v0.1.0</p>}

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
  )

  return (
    <SidebarFrame
      collapsed={collapsed}
      onToggle={onToggle}
      icon={<HexLogo className="h-7 w-7" />}
      brand={
        <div className="flex items-center gap-2.5">
          <HexLogo className="h-7 w-7 flex-shrink-0" />
          <span className="text-base font-semibold tracking-tight truncate">Veronex</span>
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
