'use client'

import './globals.css'
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { useState, useEffect } from 'react'
import { usePathname, useRouter } from 'next/navigation'
import Nav from '@/components/nav'
import { I18nProvider } from '@/components/i18n-provider'
import { ThemeProvider } from '@/components/theme-provider'
import { isLoggedIn } from '@/lib/auth'
import { api } from '@/lib/api'
import { TimezoneProvider } from '@/components/timezone-provider'
import { LabSettingsProvider } from '@/components/lab-settings-provider'
import { Nav404Provider } from '@/components/nav-404-context'
import { NavigationProgressProvider } from '@/components/nav-progress'
import { AppShell } from '@/components/layout/AppShell'
import { HexLogo } from '@/components/nav-icons'
import { TimeRangeProvider } from '@/components/time-range-context'
import { serversQuery } from '@/lib/queries'
import { STALE_TIME_FAST } from '@/lib/constants'

const NAV_COLLAPSED_KEY = 'nav-collapsed'

function AuthShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const router = useRouter()
  const queryClient = useQueryClient()
  const isLoginPage = pathname === '/login'
  const isSetupPage = pathname === '/setup'

  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)

  // Restore collapsed state from localStorage
  useEffect(() => {
    if (localStorage.getItem(NAV_COLLAPSED_KEY) === 'true') setCollapsed(true)
  }, [])

  // Close mobile nav on route change
  useEffect(() => { setMobileOpen(false) }, [pathname])

  // Prefetch server list for authenticated shell
  useEffect(() => {
    if (!isLoginPage && !isSetupPage && isLoggedIn()) {
      queryClient.prefetchQuery(serversQuery())
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryClient, isLoginPage, isSetupPage])

  // Setup / auth redirect
  useEffect(() => {
    api.setupStatus().then(({ needs_setup }) => {
      if (needs_setup) {
        if (!isSetupPage) router.replace('/setup')
      } else {
        if (isSetupPage) {
          router.replace(isLoggedIn() ? '/' : '/login')
        } else if (!isLoginPage && !isLoggedIn()) {
          router.replace('/login')
        }
      }
    }).catch(() => {
      if (!isSetupPage && !isLoginPage && !isLoggedIn()) {
        router.replace('/login')
      }
    })
  }, [isLoginPage, isSetupPage, router])

  function toggleCollapsed() {
    setCollapsed((v) => {
      const next = !v
      localStorage.setItem(NAV_COLLAPSED_KEY, String(next))
      return next
    })
  }

  if (isLoginPage || isSetupPage) {
    return <>{children}</>
  }

  return (
    <NavigationProgressProvider>
      <TimeRangeProvider>
      <AppShell
        mobileBrand={
          <>
            <HexLogo className="h-6 w-6 flex-shrink-0" />
            <span className="text-sm font-semibold tracking-tight">Veronex</span>
          </>
        }
        mobileOpen={mobileOpen}
        onMobileToggle={() => setMobileOpen((v) => !v)}
        onMobileClose={() => setMobileOpen(false)}
        sidebarWidth={collapsed ? 'md:w-14' : 'md:w-56'}
        sidebar={<Nav collapsed={collapsed} onToggle={toggleCollapsed} />}
      >
        {children}
      </AppShell>
      </TimeRangeProvider>
    </NavigationProgressProvider>
  )
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const [queryClient] = useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: STALE_TIME_FAST,
        retry: 1,
        refetchOnWindowFocus: false,
      },
    },
  }))

  return (
    <html lang="en" className="h-full" suppressHydrationWarning>
      <head>
        <title>Veronex</title>
        <meta name="description" content="Veronex — LLM inference queue and routing dashboard" />
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
        <link rel="icon" href="/favicon-light.svg" type="image/svg+xml" media="(prefers-color-scheme: light)" />
        <link rel="icon" href="/favicon-dark.svg"  type="image/svg+xml" media="(prefers-color-scheme: dark)" />
        {/* Prevent flash of wrong theme */}
        <script dangerouslySetInnerHTML={{ __html: `(function(){try{var t=localStorage.getItem('hg-theme');if(t==='dark'){document.documentElement.setAttribute('data-theme','dark');}}catch(e){}})();` }} />
      </head>
      <body className="h-full bg-background text-foreground" suppressHydrationWarning>
        <ThemeProvider>
          <I18nProvider>
            <TimezoneProvider>
              <QueryClientProvider client={queryClient}>
                <LabSettingsProvider>
                  <Nav404Provider>
                    <AuthShell>{children}</AuthShell>
                  </Nav404Provider>
                </LabSettingsProvider>
              </QueryClientProvider>
            </TimezoneProvider>
          </I18nProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
