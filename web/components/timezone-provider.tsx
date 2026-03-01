'use client'

import { createContext, useContext, useState, useCallback } from 'react'

/** Well-known preset timezones shown in the selector. */
export type PresetTimezone =
  | 'UTC'
  | 'America/New_York'
  | 'America/Chicago'
  | 'America/Denver'
  | 'America/Los_Angeles'
  | 'Europe/London'
  | 'Africa/Johannesburg'
  | 'Asia/Seoul'
  | 'Asia/Tokyo'
  | 'Australia/Sydney'
  | 'Pacific/Auckland'

/**
 * Any valid IANA timezone string (preset or custom).
 * Custom timezones are validated via Intl.DateTimeFormat before being accepted.
 */
export type Timezone = PresetTimezone | (string & {})

export const PRESET_TIMEZONES: readonly PresetTimezone[] = [
  'UTC',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'Europe/London',
  'Africa/Johannesburg',
  'Asia/Seoul',
  'Asia/Tokyo',
  'Australia/Sydney',
  'Pacific/Auckland',
]

const COOKIE_KEY = 'veronex-tz'

/** Returns true if the string is a valid IANA timezone accepted by Intl. */
export function isValidTimezone(tz: string): boolean {
  try {
    Intl.DateTimeFormat(undefined, { timeZone: tz })
    return true
  } catch {
    return false
  }
}

/** Locale → timezone default when no cookie is set. */
function localeDefault(locale: string | null): Timezone {
  switch (locale) {
    case 'ko': return 'Asia/Seoul'
    case 'ja': return 'Asia/Tokyo'
    default:   return 'America/New_York'   // en + any other locale → US Eastern (Washington DC)
  }
}

function readCookie(): Timezone | null {
  if (typeof document === 'undefined') return null
  const match = document.cookie.match(/(?:^|;\s*)veronex-tz=([^;]*)/)
  if (!match) return null
  const v = decodeURIComponent(match[1])
  return isValidTimezone(v) ? v : null
}

function writeCookie(tz: Timezone) {
  const expires = new Date(Date.now() + 365 * 864e5).toUTCString()
  document.cookie = `${COOKIE_KEY}=${encodeURIComponent(tz)}; path=/; expires=${expires}; SameSite=Lax`
}

function deleteCookie() {
  document.cookie = `${COOKIE_KEY}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Lax`
}

function initialTimezone(): Timezone {
  const cookie = readCookie()
  if (cookie) return cookie
  // No explicit override — derive from saved locale preference
  const locale = typeof localStorage !== 'undefined' ? localStorage.getItem('hg-lang') : null
  return localeDefault(locale)
}

const TimezoneContext = createContext<{
  tz: Timezone
  /** Call when user explicitly selects a timezone (persists to cookie). */
  setTz: (tz: Timezone) => void
  /**
   * Call when language changes without an explicit timezone selection.
   * Resets to locale-based default ONLY if the user has not overridden the
   * timezone themselves (no cookie present).
   */
  resetToLocaleDefault: (locale: string) => void
}>({ tz: 'America/New_York', setTz: () => {}, resetToLocaleDefault: () => {} })

export function TimezoneProvider({ children }: { children: React.ReactNode }) {
  const [tz, setTzState] = useState<Timezone>(() => initialTimezone())

  const setTz = useCallback((next: Timezone) => {
    writeCookie(next)
    setTzState(next)
  }, [])

  const resetToLocaleDefault = useCallback((locale: string) => {
    // Only update if the user hasn't explicitly set a timezone (no cookie)
    if (readCookie() !== null) return
    const next = localeDefault(locale)
    setTzState(next)
  }, [])

  return (
    <TimezoneContext.Provider value={{ tz, setTz, resetToLocaleDefault }}>
      {children}
    </TimezoneContext.Provider>
  )
}

export function useTimezone() {
  return useContext(TimezoneContext)
}
