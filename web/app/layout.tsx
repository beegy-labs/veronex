'use client'

import './globals.css'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState } from 'react'
import Nav from '@/components/nav'

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
    <html lang="en" className="h-full dark">
      <head>
        <title>InferQ</title>
        <meta name="description" content="InferQ — LLM inference queue and routing dashboard" />
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
      </head>
      <body className="h-full bg-slate-950 text-slate-100">
        <QueryClientProvider client={queryClient}>
          <div className="flex h-full min-h-screen">
            <Nav />
            <main className="flex-1 overflow-auto p-8">
              {children}
            </main>
          </div>
        </QueryClientProvider>
      </body>
    </html>
  )
}
