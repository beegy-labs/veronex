'use client'

import Link from 'next/link'
import { usePathname, useSearchParams } from 'next/navigation'
import { useState, useEffect, Suspense } from 'react'
import {
  LayoutDashboard, List, Key, Server,
  BarChart2, Gauge, Sun, Moon, ChevronLeft, Languages, Clock,
  BookOpen, HardDrive, Sparkles, ChevronDown, Menu,
  Users, Shield, LogOut, Workflow, Settings2, FlaskConical,
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
import { getAuthUser } from '@/lib/auth'
import { redirectToLogin } from '@/lib/auth-guard'
import { api } from '@/lib/api'
import { useLabSettings } from '@/components/lab-settings-provider'
import { Switch } from '@/components/ui/switch'
import { useTimezone, type Timezone, PRESET_TIMEZONES, isValidTimezone } from '@/components/timezone-provider'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'

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
  section?: string  // if set: matched via ?s= query param; otherwise: pathname === href
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
  {
    type: 'group',
    id: 'overview',
    labelKey: 'nav.monitor',
    icon: LayoutDashboard,
    basePath: '/overview',
    children: [
      { href: '/overview',     labelKey: 'nav.dashboard',   icon: LayoutDashboard },
      { href: '/usage',        labelKey: 'nav.usage',       icon: BarChart2 },
      { href: '/performance',  labelKey: 'nav.performance', icon: Gauge },
    ],
  },
  {
    type: 'group',
    id: 'jobs',
    labelKey: 'nav.jobs',
    icon: List,
    basePath: '/jobs',
    children: [
      { href: '/jobs',  labelKey: 'nav.jobs',  icon: List },
      { href: '/flow',  labelKey: 'nav.flow',  icon: Workflow },
    ],
  },
  { type: 'link', href: '/keys',    labelKey: 'nav.keys',    icon: Key },
  { type: 'link', href: '/servers', labelKey: 'nav.servers', icon: HardDrive },
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
  const { tz, setTz, resetToLocaleDefault } = useTimezone()

  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  const [locale, setLocale] = useState<Locale>('en')
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({})
  const [authUser, setAuthUser] = useState<{ username: string; role: string } | null>(null)
  const { labSettings, refetch: refetchLabSettings } = useLabSettings()
  const [showSettings, setShowSettings] = useState(false)
  const [showCustomTzInline, setShowCustomTzInline] = useState(false)
  const [customTzInput, setCustomTzInput] = useState('')
  const [customTzError, setCustomTzError] = useState(false)
  const [labLoading, setLabLoading] = useState(false)

  const isPresetTz = PRESET_TIMEZONES.includes(tz as typeof PRESET_TIMEZONES[number])
  const tzSelectValue = isPresetTz ? tz : '__custom__'

  // Restore persisted state on mount
  useEffect(() => {
    const user = getAuthUser()
    setAuthUser(user)

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

  function changeLocale(next: Locale) {
    setLocale(next)
    localStorage.setItem(localStorageKey, next)
    i18n.changeLanguage(next)
    // Auto-set timezone from locale if user hasn't explicitly chosen one
    resetToLocaleDefault(next)
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
          className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          title="Menu"
        >
          <Menu className="h-5 w-5" />
        </button>
        <HexLogo className="h-6 w-6 flex-shrink-0" />
        <span className="text-sm font-semibold tracking-tight">Veronex</span>
      </div>

      {/* ── Backdrop ───────────────────────────────────────────────── */}
      {mobileOpen && (
        <div
          className="md:hidden fixed inset-0 z-40 bg-black/50"
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
          'fixed inset-y-0 left-0 z-50 w-72',
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
        {navItems
          .map(item =>
            item.type === 'group' && item.id === 'providers'
              ? {
                  ...item,
                  children: item.children.filter(c =>
                    c.section !== 'gemini' || (labSettings?.gemini_function_calling ?? false)
                  ),
                }
              : item
          )
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
            {authUser.role === 'super' && (
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
                title="Sign out"
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
            onClick={() => { setShowSettings(true); setShowCustomTzInline(false) }}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
            title={t('common.settings')}
          >
            <Settings2 className="h-4 w-4" />
          </button>

          <button
            type="button"
            onClick={toggleTheme}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
            title={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
          >
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>
        </div>

        {/* Settings dialog — language + timezone */}
        {showSettings && (
          <Dialog open onOpenChange={(open) => {
            if (!open) { setShowSettings(false); setShowCustomTzInline(false); setCustomTzError(false) }
          }}>
            <DialogContent className="max-w-xs">
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <Settings2 className="h-4 w-4 text-primary" />
                  {t('common.settings')}
                </DialogTitle>
              </DialogHeader>

              <div className="space-y-4 pt-1">
                {/* Language row */}
                <div className="flex items-center gap-3">
                  <Languages className="h-4 w-4 text-muted-foreground shrink-0" />
                  <span className="text-sm text-muted-foreground flex-1">{t('common.language')}</span>
                  <Select value={locale} onValueChange={(v) => changeLocale(v as Locale)}>
                    <SelectTrigger className="h-8 w-36 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {locales.map((loc) => (
                        <SelectItem key={loc} value={loc} className="text-xs">
                          {localeLabels[loc]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Timezone row */}
                <div className="flex items-center gap-3">
                  <Clock className="h-4 w-4 text-muted-foreground shrink-0" />
                  <span className="text-sm text-muted-foreground flex-1">{t('common.timezone')}</span>
                  <Select
                    value={tzSelectValue}
                    onValueChange={(v) => {
                      if (v === '__custom__') {
                        setCustomTzInput(isPresetTz ? '' : tz)
                        setCustomTzError(false)
                        setShowCustomTzInline(true)
                      } else {
                        setTz(v as Timezone)
                        setShowCustomTzInline(false)
                      }
                    }}
                  >
                    <SelectTrigger className="h-8 w-36 text-xs">
                      {isPresetTz
                        ? <SelectValue />
                        : <span className="truncate">{tz.split('/').pop()}</span>
                      }
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="UTC" className="text-xs">{t('common.utc')}</SelectItem>
                      <SelectItem value="America/New_York" className="text-xs">{t('common.eastern')}</SelectItem>
                      <SelectItem value="America/Chicago" className="text-xs">{t('common.central')}</SelectItem>
                      <SelectItem value="America/Denver" className="text-xs">{t('common.mountain')}</SelectItem>
                      <SelectItem value="America/Los_Angeles" className="text-xs">{t('common.pacific')}</SelectItem>
                      <SelectItem value="Europe/London" className="text-xs">{t('common.london')}</SelectItem>
                      <SelectItem value="Africa/Johannesburg" className="text-xs">{t('common.johannesburg')}</SelectItem>
                      <SelectItem value="Asia/Seoul" className="text-xs">{t('common.kst')}</SelectItem>
                      <SelectItem value="Asia/Tokyo" className="text-xs">{t('common.jst')}</SelectItem>
                      <SelectItem value="Australia/Sydney" className="text-xs">{t('common.sydney')}</SelectItem>
                      <SelectItem value="Pacific/Auckland" className="text-xs">{t('common.auckland')}</SelectItem>
                      <SelectItem value="__custom__" className="text-xs">{t('common.custom')}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                {/* Lab features section */}
                <div className="border-t pt-3 mt-1">
                  <div className="flex items-center gap-2 mb-2">
                    <FlaskConical className="h-4 w-4 text-amber-500 shrink-0" />
                    <span className="text-sm font-medium flex-1">{t('common.labFeatures')}</span>
                    <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-400 uppercase tracking-wide">
                      Lab
                    </span>
                  </div>
                  <p className="text-xs text-muted-foreground mb-3 pl-6">{t('common.labFeaturesDesc')}</p>

                  {/* Gemini function calling */}
                  <div className="pl-6 space-y-1">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex-1 min-w-0">
                        <p className="text-xs font-medium">{t('common.labGeminiFunctionCalling')}</p>
                        <p className="text-[11px] text-muted-foreground leading-snug mt-0.5">{t('common.labGeminiFunctionCallingDesc')}</p>
                      </div>
                      <Switch
                        checked={labSettings?.gemini_function_calling ?? false}
                        disabled={labLoading || labSettings === null}
                        onCheckedChange={async (checked) => {
                          setLabLoading(true)
                          try {
                            await api.patchLabSettings({ gemini_function_calling: checked })
                            await refetchLabSettings()
                          } catch {
                            // keep previous state on error
                          } finally {
                            setLabLoading(false)
                          }
                        }}
                      />
                    </div>
                  </div>
                </div>

                {/* Custom IANA input — shown inline when "Custom…" is selected */}
                {showCustomTzInline && (
                  <div className="pl-7 space-y-2">
                    <Input
                      value={customTzInput}
                      onChange={(e) => { setCustomTzInput(e.target.value); setCustomTzError(false) }}
                      placeholder={t('common.customTimezonePlaceholder')}
                      className="font-mono text-xs h-8"
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') {
                          if (isValidTimezone(customTzInput.trim())) {
                            setTz(customTzInput.trim() as Timezone)
                            setShowCustomTzInline(false)
                          } else {
                            setCustomTzError(true)
                          }
                        }
                      }}
                    />
                    <p className="text-xs text-muted-foreground">{t('common.customTimezoneHint')}</p>
                    {customTzError && (
                      <p className="text-xs text-destructive">{t('common.customTimezoneInvalid')}</p>
                    )}
                    <div className="flex gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 text-xs flex-1"
                        onClick={() => { setShowCustomTzInline(false); setCustomTzError(false) }}
                      >
                        {t('common.cancel')}
                      </Button>
                      <Button
                        size="sm"
                        className="h-7 text-xs flex-1"
                        onClick={() => {
                          if (isValidTimezone(customTzInput.trim())) {
                            setTz(customTzInput.trim() as Timezone)
                            setShowCustomTzInline(false)
                          } else {
                            setCustomTzError(true)
                          }
                        }}
                      >
                        {t('common.save')}
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            </DialogContent>
          </Dialog>
        )}
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
