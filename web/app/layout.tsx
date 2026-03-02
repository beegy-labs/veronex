'use client'

import './globals.css'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState, useEffect } from 'react'
import { usePathname, useRouter } from 'next/navigation'
import Nav from '@/components/nav'
import { I18nProvider } from '@/components/i18n-provider'
import { ThemeProvider } from '@/components/theme-provider'
import { isLoggedIn } from '@/lib/auth'
import { api } from '@/lib/api'
import { TimezoneProvider } from '@/components/timezone-provider'
import { LabSettingsProvider } from '@/components/lab-settings-provider'

function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const router = useRouter()
  const isLoginPage = pathname === '/login'
  const isSetupPage = pathname === '/setup'

  useEffect(() => {
    api.setupStatus().then(({ needs_setup }) => {
      if (needs_setup) {
        // Setup not complete — only /setup is allowed
        if (!isSetupPage) router.replace('/setup')
      } else {
        // Setup complete — /setup must not be accessible
        if (isSetupPage) {
          router.replace(isLoggedIn() ? '/' : '/login')
        } else if (!isLoginPage && !isLoggedIn()) {
          router.replace('/login')
        }
      }
    }).catch(() => {
      // API unreachable — fall back to auth check (don't redirect to setup)
      if (!isSetupPage && !isLoginPage && !isLoggedIn()) {
        router.replace('/login')
      }
    })
  }, [isLoginPage, isSetupPage, router])

  if (isLoginPage || isSetupPage) {
    return <>{children}</>
  }

  return (
    <div className="flex h-full min-h-screen">
      <Nav />
      <main className="flex-1 overflow-auto p-4 pt-16 md:p-8">
        {children}
      </main>
    </div>
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
        staleTime: 30_000,
        retry: 1,
        refetchOnWindowFocus: false,
      },
    },
  }))

  return (
    <html lang="en" className="h-full">
      <head>
        <title>Veronex</title>
        <meta name="description" content="Veronex — LLM inference queue and routing dashboard" />
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
        {/* Prevent flash of wrong theme */}
        <script dangerouslySetInnerHTML={{ __html: `(function(){try{var t=localStorage.getItem('hg-theme');if(t==='dark'){document.documentElement.setAttribute('data-theme','dark');}}catch(e){}})();` }} />
      </head>
      <body className="h-full bg-background text-foreground">
        <ThemeProvider>
          <I18nProvider>
            <TimezoneProvider>
              <QueryClientProvider client={queryClient}>
                <LabSettingsProvider>
                  <AppShell>{children}</AppShell>
                </LabSettingsProvider>
              </QueryClientProvider>
            </TimezoneProvider>
          </I18nProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
