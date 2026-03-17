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
    // Cancel any pending idle-transition timeout from a previous finish()
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
    // Guard: ignore spurious finish() calls when no start() was issued
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

  // Hard reset — used when navigating without a click (router.replace / redirect)
  const reset = useCallback(() => {
    clearAll()
    countRef.current = 0
    pctRef.current = 0
    setPct(0)
    setPhase('idle')
  }, [])

  useEffect(() => () => clearAll(), [])

  return { pct, phase, start, finish, reset }
}

// ── HoneycombBar ──────────────────────────────────────────────────────────────
//
// Flat-top hexagonal tiling pattern (R=7, cell W=14, H≈12):
//   Center hex at (10.5, 6) + partial hexes at all 4 corners tile seamlessly.
//   The pattern y offset (-2) centers cells vertically in the 8px bar.
//
const HEX_POLYS = [
  '17.5,6 14,0 7,0 3.5,6 7,12 14,12',
  '7,0 3.5,-6 -3.5,-6 -7,0 -3.5,6 3.5,6',
  '28,0 24.5,-6 17.5,-6 14,0 17.5,6 24.5,6',
  '7,12 3.5,6 -3.5,6 -7,12 -3.5,18 3.5,18',
  '28,12 24.5,6 17.5,6 14,12 17.5,18 24.5,18',
]

// IDs passed as props so React.useId() ensures DOM uniqueness per instance
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
      {/* Track — subtle honeycomb grid */}
      <svg
        className="absolute inset-0 text-border"
        width="100%"
        height="8"
        xmlns="http://www.w3.org/2000/svg"
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

      {/* Fill — primary color, reveals left-to-right via clip-path */}
      <svg
        className="absolute inset-0 text-primary"
        width="100%"
        height="8"
        xmlns="http://www.w3.org/2000/svg"
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

      {/* Leading-edge glow */}
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
  const { pct, phase, start, finish, reset } = useProgressMachine()

  // React.useId() returns strings like ":r1:" — colons are invalid in SVG/XML NCNames.
  // Strip them so the pattern id is a valid XML identifier per the SVG spec.
  const rawId = useId()
  const safeId = rawId.replace(/:/g, '')
  const trackId = `vx-track-${safeId}`
  const fillId  = `vx-fill-${safeId}`

  const prevPathnameRef = useRef(pathname)
  // Track queries that have started loading so we only finish when all are done
  const pendingQueriesRef = useRef(new Set<string>())

  // Navigation: detect clicks on internal links
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      const anchor = (e.target as Element).closest('a')
      if (!anchor?.href) return
      const url = new URL(anchor.href, window.location.href)
      if (url.origin !== window.location.origin) return
      if (url.pathname === window.location.pathname && url.search === window.location.search) return
      if (anchor.target === '_blank') return
      start()
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [start])

  // Navigation: finish/reset when pathname changes
  useEffect(() => {
    if (pathname !== prevPathnameRef.current) {
      prevPathnameRef.current = pathname
      // Check size BEFORE clearing — .clear() makes size always 0 afterward
      const hadPending = pendingQueriesRef.current.size > 0
      pendingQueriesRef.current.clear()
      if (phase === 'running') {
        finish() // click-triggered nav: complete the bar animation
      } else if (hadPending) {
        reset()  // programmatic nav with stale pending queries: hard reset countRef
      }
    }
  }, [pathname, phase, finish, reset])

  // Data loading: track initial (never-fetched) React Query requests
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
