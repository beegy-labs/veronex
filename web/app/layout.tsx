'use client'

/* ============================================================================
   verodesign 통합 — CSS 로드 순서가 매우 중요. 다음 순서를 변경하지 말 것.

   reset → core (primitives + base) → theme (light-dark()) → utilities
   → state-variants → responsive → animations → app overrides → app extras
   ============================================================================ */
// verodesign CSS copied into app/styles/vds — Next 16 Turbopack does not
// resolve package.json `exports` for CSS through symlinked node_modules,
// so we ship the bundled CSS as in-tree assets.
import './styles/vds/full.css'
import './styles/vds/theme-veronex.css'
import './styles/vds/state-variants.css'
import './styles/vds/responsive.css'
import './styles/vds/animations.css'
import './globals.css'
import './swagger-overrides.css'

import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { useState, useEffect } from 'react'
import { usePathname, useRouter } from 'next/navigation'
import Nav from '@/components/nav'
import { I18nProvider } from '@/components/i18n-provider'
import { ThemeProvider, themeInitScript } from '@/components/theme-provider'
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
import { useTranslation } from '@/i18n'

const NAV_COLLAPSED_KEY = 'nav-collapsed'

function AuthShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const router = useRouter()
  const queryClient = useQueryClient()
  const { t } = useTranslation()
  const isLoginPage = pathname === '/login'
  const isSetupPage = pathname === '/setup'

  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)

  useEffect(() => {
    if (localStorage.getItem(NAV_COLLAPSED_KEY) === 'true') setCollapsed(true)
  }, [])

  useEffect(() => { setMobileOpen(false) }, [pathname])

  useEffect(() => {
    if (!isLoginPage && !isSetupPage && isLoggedIn()) {
      queryClient.prefetchQuery(serversQuery())
    }
  }, [queryClient, isLoginPage, isSetupPage])

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

  // Cmd/Ctrl+B → toggle sidebar
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
        e.preventDefault()
        toggleCollapsed()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

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
        <a href="#main-content" className="skip-link">
          {t('common.skipToMain', '메인 콘텐츠로 건너뛰기')}
        </a>
        <AppShell
          mobileBrand={
            <>
              <HexLogo className="vds-h-6 vds-w-6 vds-flex-shrink-0" />
              <span className="vds-text-sm vds-font-600 vds-tracking-tight">Veronex</span>
            </>
          }
          mobileOpen={mobileOpen}
          onMobileToggle={() => setMobileOpen((v) => !v)}
          onMobileClose={() => setMobileOpen(false)}
          collapsed={collapsed}
          sidebar={<Nav collapsed={collapsed} onToggle={toggleCollapsed} />}
        >
          <main id="main-content" tabIndex={-1}>
            {children}
          </main>
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
    <html lang="ko" className="vds-h-full" data-theme="veronex" data-mode="light" suppressHydrationWarning>
      <head>
        <title>Veronex</title>
        <meta name="description" content="Veronex — LLM inference queue and routing dashboard" />
        <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
        <link rel="icon" href="/favicon-light.svg" type="image/svg+xml" media="(prefers-color-scheme: light)" />
        <link rel="icon" href="/favicon-dark.svg"  type="image/svg+xml" media="(prefers-color-scheme: dark)" />
        <script dangerouslySetInnerHTML={{ __html: themeInitScript }} />
      </head>
      <body className="vds-h-full" suppressHydrationWarning>
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
