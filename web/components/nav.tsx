'use client'

import Link from 'next/link'
import { usePathname, useSearchParams } from 'next/navigation'
import { useState, useEffect, Suspense } from 'react'
import {
  LayoutDashboard, List, Key, FlaskConical, Server,
  BarChart2, Gauge, Sun, Moon, ChevronLeft, Languages,
  BookOpen, HardDrive, Sparkles, ChevronDown,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { useTheme } from '@/components/theme-provider'
import { useTranslation } from '@/i18n'
import { i18n } from '@/i18n'
import { locales, localeLabels, localStorageKey, type Locale } from '@/i18n/config'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

// ── Constants ──────────────────────────────────────────────────────────────────

const NAV_COLLAPSED_KEY = 'nav-collapsed'
const groupStorageKey = (id: string) => `nav-group-${id}`

// ── Nav item types ──────────────────────────────────────────────────────────────

type NavLink = {
  type: 'link'
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
}

type NavGroupChild = {
  href: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  section: string
}

type NavGroup = {
  type: 'group'
  id: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }>
  basePath: string
  children: NavGroupChild[]
}

type NavItem = NavLink | NavGroup

// ── Nav structure ───────────────────────────────────────────────────────────────
// Add new providers here — sub-items appear automatically in the sidebar.

const navItems: NavItem[] = [
  { type: 'link', href: '/overview',    labelKey: 'nav.overview',    icon: LayoutDashboard },
  { type: 'link', href: '/jobs',        labelKey: 'nav.jobs',        icon: List },
  { type: 'link', href: '/keys',        labelKey: 'nav.keys',        icon: Key },
  { type: 'link', href: '/servers',     labelKey: 'nav.servers',     icon: HardDrive },
  {
    type: 'group',
    id: 'providers',
    labelKey: 'nav.providers',
    icon: Server,
    basePath: '/providers',
    children: [
      { href: '/providers?s=ollama', labelKey: 'nav.ollama', icon: OllamaIcon, section: 'ollama' },
      { href: '/providers?s=gemini', labelKey: 'nav.gemini', icon: Sparkles,   section: 'gemini' },
    ],
  },
  { type: 'link', href: '/usage',       labelKey: 'nav.usage',       icon: BarChart2 },
  { type: 'link', href: '/performance', labelKey: 'nav.performance', icon: Gauge },
  { type: 'link', href: '/api-test',    labelKey: 'nav.test',        icon: FlaskConical },
  { type: 'link', href: '/api-docs',    labelKey: 'nav.apiDocs',     icon: BookOpen },
]

// ── Logo ───────────────────────────────────────────────────────────────────────

function HexLogo({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Veronex"
    >
      <defs>
        <linearGradient id="hex-grad" x1="2.5" y1="4.3" x2="29.5" y2="27.7" gradientUnits="userSpaceOnUse">
          <stop offset="0%"   stopColor="var(--theme-logo-start)" />
          <stop offset="100%" stopColor="var(--theme-logo-end)" />
        </linearGradient>
      </defs>
      <polygon
        points="29.5,16 22.8,27.7 9.2,27.7 2.5,16 9.2,4.3 22.8,4.3"
        fill="url(#hex-grad)"
      />
      <polygon
        points="25,16 20.5,23.8 11.5,23.8 7,16 11.5,8.2 20.5,8.2"
        fill="none"
        stroke="white"
        strokeWidth="1.5"
        strokeOpacity="0.55"
      />
    </svg>
  )
}

// ── Ollama llama logo (matches Ollama brand) ────────────────────────────────────

function OllamaIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="currentColor"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Ollama"
    >
      {/* Left ear */}
      <path d="M7.5 1.5 C7 1.5 6.5 2 6.5 2.5 L6.5 5 C6.5 5.5 7 6 7.5 6 L9 6 C9.5 6 10 5.5 10 5 L10 2.5 C10 2 9.5 1.5 9 1.5 Z" />
      {/* Right ear */}
      <path d="M15 1.5 C14.5 1.5 14 2 14 2.5 L14 5 C14 5.5 14.5 6 15 6 L16.5 6 C17 6 17.5 5.5 17.5 5 L17.5 2.5 C17.5 2 17 1.5 16.5 1.5 Z" />
      {/* Head */}
      <ellipse cx="12" cy="9" rx="5.5" ry="4.5" />
      {/* Neck */}
      <path d="M9.5 13 L9.5 16 C9.5 16.5 10 17 10.5 17 L13.5 17 C14 17 14.5 16.5 14.5 16 L14.5 13 Z" />
      {/* Body */}
      <rect x="6.5" y="16.5" width="11" height="6" rx="3" />
    </svg>
  )
}

// ── Inner nav (needs useSearchParams — wrapped in Suspense by parent) ───────────

function NavContent() {
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const { theme, toggleTheme } = useTheme()
  const { t } = useTranslation()

  const [collapsed, setCollapsed] = useState(false)
  const [locale, setLocale] = useState<Locale>('en')
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({})

  // Restore persisted state on mount
  useEffect(() => {
    const savedCollapsed = localStorage.getItem(NAV_COLLAPSED_KEY)
    if (savedCollapsed === 'true') setCollapsed(true)

    const savedLocale = localStorage.getItem(localStorageKey) as Locale | null
    if (savedLocale && locales.includes(savedLocale)) setLocale(savedLocale)
    else {
      const browser = navigator.language.slice(0, 2) as Locale
      if (locales.includes(browser)) setLocale(browser)
    }

    const groups: Record<string, boolean> = {}
    for (const item of navItems) {
      if (item.type === 'group') {
        const saved = localStorage.getItem(groupStorageKey(item.id))
        groups[item.id] = saved !== null ? saved === 'true' : false
      }
    }
    setOpenGroups(groups)
  }, [])

  // Auto-open the group containing the active route
  useEffect(() => {
    for (const item of navItems) {
      if (item.type === 'group' && pathname.startsWith(item.basePath)) {
        setOpenGroups((prev) => {
          if (prev[item.id]) return prev
          const next = { ...prev, [item.id]: true }
          localStorage.setItem(groupStorageKey(item.id), 'true')
          return next
        })
      }
    }
  }, [pathname])

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

  function changeLocale(next: Locale) {
    setLocale(next)
    localStorage.setItem(localStorageKey, next)
    i18n.changeLanguage(next)
  }

  function isSubActive(section: string, basePath: string): boolean {
    if (pathname !== basePath) return false
    const current = searchParams.get('s') ?? 'ollama'
    return current === section
  }

  function isGroupActive(item: NavGroup): boolean {
    return pathname.startsWith(item.basePath)
  }

  return (
    <aside
      className={cn(
        'flex-shrink-0 bg-card border-r border-border flex flex-col transition-all duration-200',
        collapsed ? 'w-14' : 'w-56',
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
            title="Expand"
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
              title="Collapse"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
          </>
        )}
      </div>

      {/* ── Nav links ──────────────────────────────────────────────── */}
      <nav className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto">
        {navItems.map((item) => {
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
                    const active = isSubActive(child.section, item.basePath)
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
        <div className={cn(
          'flex items-center gap-1 px-1',
          collapsed ? 'justify-center' : 'justify-between',
        )}>
          {!collapsed && (
            <p className="text-xs text-muted-foreground shrink-0">v0.1.0</p>
          )}

          <Select value={locale} onValueChange={(v) => changeLocale(v as Locale)}>
            <SelectTrigger
              className="h-7 gap-1 border-0 bg-transparent px-1.5 text-[11px] font-medium text-muted-foreground hover:text-foreground hover:bg-accent focus:ring-0 focus:ring-offset-0 w-auto min-w-0"
              title="Language"
            >
              <Languages className="h-3.5 w-3.5 shrink-0" />
              {!collapsed && <SelectValue />}
            </SelectTrigger>
            <SelectContent side="top" align="start">
              {locales.map((loc) => (
                <SelectItem key={loc} value={loc} className="text-xs">
                  {localeLabels[loc]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <button
            type="button"
            onClick={toggleTheme}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
            title={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
          >
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>
        </div>
      </div>
    </aside>
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
