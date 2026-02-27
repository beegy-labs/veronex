'use client'

import './globals.css'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState } from 'react'
import Nav from '@/components/nav'
import { I18nProvider } from '@/components/i18n-provider'
import { ThemeProvider } from '@/components/theme-provider'

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
            <QueryClientProvider client={queryClient}>
              <div className="flex h-full min-h-screen">
                <Nav />
                <main className="flex-1 overflow-auto p-4 pt-16 md:p-8">
                  {children}
                </main>
              </div>
            </QueryClientProvider>
          </I18nProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
