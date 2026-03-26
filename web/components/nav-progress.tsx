'use client'

import { memo, useCallback, useEffect, useId, useRef, useState } from 'react'
import { usePathname } from 'next/navigation'
import { useQueryClient } from '@tanstack/react-query'
import { tokens } from '@/lib/design-tokens'

// ── Progress machine ──────────────────────────────────────────────────────────

type Phase = 'idle' | 'running' | 'finishing'

function useProgressMachine() {
  const [pct, setPct] = useState(0)
  const [phase, setPhase] = useState<Phase>('idle')
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const countRef = useRef(0)
  const pctRef = useRef(0)

  const clearAll = () => {
    if (timerRef.current !== null) { clearInterval(timerRef.current); timerRef.current = null }
    if (timeoutRef.current !== null) { clearTimeout(timeoutRef.current); timeoutRef.current = null }
  }

  const start = useCallback(() => {
    countRef.current++
    if (countRef.current > 1) return // already running
    clearAll()
    pctRef.current = 0
    setPct(0)
    setPhase('running')
    timerRef.current = setInterval(() => {
      // Exponential approach: crawls toward 88%, slows as it gets closer
      pctRef.current = pctRef.current + (88 - pctRef.current) * 0.04
      setPct(pctRef.current)
    }, 80)
  }, [])

  const finish = useCallback(() => {
    if (countRef.current <= 0) return
    countRef.current--
    if (countRef.current > 0) return // other sources still loading
    if (timerRef.current !== null) { clearInterval(timerRef.current); timerRef.current = null }
    setPhase('finishing')
    setPct(100)
    timeoutRef.current = setTimeout(() => {
      timeoutRef.current = null
      setPhase('idle')
      setPct(0)
      pctRef.current = 0
    }, 450)
  }, [])

  // Hard reset — force-completes the bar regardless of countRef
  const done = useCallback(() => {
    clearAll()
    countRef.current = 0
    setPhase('finishing')
    setPct(100)
    timeoutRef.current = setTimeout(() => {
      timeoutRef.current = null
      setPhase('idle')
      setPct(0)
      pctRef.current = 0
    }, 450)
  }, [])

  const reset = useCallback(() => {
    clearAll()
    countRef.current = 0
    pctRef.current = 0
    setPct(0)
    setPhase('idle')
  }, [])

  useEffect(() => () => clearAll(), [])

  return { pct, phase, start, finish, done, reset }
}

// ── HoneycombBar ──────────────────────────────────────────────────────────────

const HEX_POLYS = [
  '17.5,6 14,0 7,0 3.5,6 7,12 14,12',
  '7,0 3.5,-6 -3.5,-6 -7,0 -3.5,6 3.5,6',
  '28,0 24.5,-6 17.5,-6 14,0 17.5,6 24.5,6',
  '7,12 3.5,6 -3.5,6 -7,12 -3.5,18 3.5,18',
  '28,12 24.5,6 17.5,6 14,12 17.5,18 24.5,18',
]

interface HoneycombBarProps {
  pct: number
  visible: boolean
  trackId: string
  fillId: string
}

const HoneycombBar = memo(function HoneycombBar({ pct, visible, trackId, fillId }: HoneycombBarProps) {
  return (
    <div
      aria-hidden
      className="pointer-events-none fixed inset-x-0 top-0 z-[9999] h-[8px] overflow-hidden"
      style={{
        opacity: visible ? 1 : 0,
        transition: visible ? 'opacity 120ms ease-in' : 'opacity 450ms ease-out',
      }}
    >
      <svg
        className="absolute inset-0 text-border"
        width="100%" height="8" xmlns="http://www.w3.org/2000/svg"
      >
        <defs>
          <pattern id={trackId} x="0" y="-2" width="21" height="12" patternUnits="userSpaceOnUse">
            {HEX_POLYS.map((pts, i) => (
              <polygon key={i} points={pts} fill="none" stroke="currentColor" strokeWidth="0.7" opacity="0.35" />
            ))}
          </pattern>
        </defs>
        <rect width="100%" height="8" fill={`url(#${trackId})`} />
      </svg>

      <svg
        className="absolute inset-0 text-primary"
        width="100%" height="8" xmlns="http://www.w3.org/2000/svg"
        style={{ clipPath: `inset(0 ${100 - pct}% 0 0)` }}
      >
        <defs>
          <pattern id={fillId} x="0" y="-2" width="21" height="12" patternUnits="userSpaceOnUse">
            {HEX_POLYS.map((pts, i) => (
              <polygon key={i} points={pts} fill="currentColor" />
            ))}
          </pattern>
        </defs>
        <rect width="100%" height="8" fill={`url(#${fillId})`} />
      </svg>

      {pct > 0 && pct < 100 && (
        <div
          className="absolute top-0 bottom-0 w-14 -translate-x-1/2"
          style={{
            left: `${pct}%`,
            background: `linear-gradient(to right, transparent, color-mix(in oklch, ${tokens.brand.primary} 65%, transparent) 50%, transparent)`,
          }}
        />
      )}
    </div>
  )
})

// ── NavigationProgressProvider ────────────────────────────────────────────────

export function NavigationProgressProvider({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const queryClient = useQueryClient()
  const { pct, phase, start, finish, done, reset } = useProgressMachine()

  const rawId = useId()
  const safeId = rawId.replace(/[^a-zA-Z0-9]/g, '')
  const trackId = `vx-track-${safeId}`
  const fillId  = `vx-fill-${safeId}`

  const prevHrefRef = useRef(pathname)
  const pendingQueriesRef = useRef(new Set<string>())

  // Navigation: detect clicks on internal links
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      const anchor = (e.target as Element).closest('a')
      if (!anchor?.href) return
      const url = new URL(anchor.href, window.location.href)
      if (url.origin !== window.location.origin) return
      // Skip same-page navigations (including query param changes on same path)
      if (url.pathname === window.location.pathname) return
      if (anchor.target === '_blank') return
      start()
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [start])

  // Navigation: force-complete when pathname changes
  // done() resets countRef to 0 so stale query start/finish pairs can't leak
  useEffect(() => {
    if (pathname !== prevHrefRef.current) {
      prevHrefRef.current = pathname
      pendingQueriesRef.current.clear()
      if (phase === 'running') {
        done()
      } else {
        reset()
      }
    }
  }, [pathname, phase, done, reset])

  // Data loading: track initial (never-fetched) React Query requests
  // Only track queries that have NEVER had data (dataUpdatedAt === 0)
  // and have not errored — prevents 500 retry loops from re-triggering the bar
  useEffect(() => {
    const cache = queryClient.getQueryCache()
    const unsub = cache.subscribe((event) => {
      if (!event) return
      const { query } = event
      const key = query.queryHash

      if (
        event.type === 'updated' &&
        query.state.fetchStatus === 'fetching' &&
        query.state.dataUpdatedAt === 0 &&
        query.state.status !== 'error' &&
        !pendingQueriesRef.current.has(key)
      ) {
        pendingQueriesRef.current.add(key)
        start()
      }

      if (
        event.type === 'updated' &&
        query.state.fetchStatus === 'idle' &&
        pendingQueriesRef.current.has(key)
      ) {
        pendingQueriesRef.current.delete(key)
        finish()
      }
    })
    return unsub
  }, [queryClient, start, finish])

  return (
    <>
      <HoneycombBar pct={pct} visible={phase !== 'idle'} trackId={trackId} fillId={fillId} />
      {children}
    </>
  )
}
