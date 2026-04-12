'use client'

import { createContext, useContext, useState, useEffect } from 'react'
import type { TimeRange } from '@/components/time-range-selector'

// ── Default & storage ─────────────────────────────────────────────────────────

const STORAGE_KEY = 'veronex:timeRange'
const DEFAULT_RANGE: TimeRange = { hours: 24 }

function loadRange(): TimeRange {
  if (typeof window === 'undefined') return DEFAULT_RANGE
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return DEFAULT_RANGE
    return JSON.parse(raw) as TimeRange
  } catch {
    return DEFAULT_RANGE
  }
}

function saveRange(range: TimeRange) {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(range)) } catch {}
}

// ── Context ───────────────────────────────────────────────────────────────────

interface TimeRangeContextValue {
  range: TimeRange
  setRange: (range: TimeRange) => void
}

const TimeRangeContext = createContext<TimeRangeContextValue | null>(null)

// ── Provider ──────────────────────────────────────────────────────────────────

export function TimeRangeProvider({ children }: { children: React.ReactNode }) {
  const [range, setRangeState] = useState<TimeRange>(DEFAULT_RANGE)

  // Restore from localStorage after mount (avoids SSR mismatch)
  useEffect(() => { setRangeState(loadRange()) }, [])

  function setRange(next: TimeRange) {
    saveRange(next)
    setRangeState(next)
  }

  return (
    <TimeRangeContext value={{ range, setRange }}>
      {children}
    </TimeRangeContext>
  )
}

// ── Hook ──────────────────────────────────────────────────────────────────────

export function useTimeRange(): TimeRangeContextValue {
  const ctx = useContext(TimeRangeContext)
  if (!ctx) throw new Error('useTimeRange must be used inside TimeRangeProvider')
  return ctx
}
