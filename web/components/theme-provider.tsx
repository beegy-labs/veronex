'use client'

import { createContext, useCallback, useContext, useEffect, useState } from 'react'

/**
 * verodesign 정책: <html data-theme="veronex" data-mode="light|dark"> 두 속성으로
 * 테마/모드 분리. 색상 토큰은 light-dark() CSS 함수로 모드에 따라 자동 전환.
 *
 * localStorage:
 *   vds:theme  — "veronex" (현재 단일 theme)
 *   vds:mode   — "light" | "dark" | "auto"
 *
 * FOUC 방지: layout.tsx <head> 안에서 inline script로 attribute를 일치시킴.
 */

type Mode = 'light' | 'dark'
type Theme = Mode  // legacy alias for callers expecting `theme`

const THEME_KEY = 'vds:theme'
const MODE_KEY = 'vds:mode'
const APP_THEME = 'veronex'

interface ThemeContextValue {
  /** Resolved mode currently applied to <html>. */
  mode: Mode
  /** Legacy alias — same value as `mode`. */
  theme: Theme
  /** Switch between light and dark. */
  toggleTheme: () => void
  /** Force a specific mode. */
  setMode: (m: Mode) => void
}

const ThemeContext = createContext<ThemeContextValue>({
  mode: 'light',
  theme: 'light',
  toggleTheme: () => {},
  setMode: () => {},
})

function readInitialMode(): Mode {
  if (typeof window === 'undefined') return 'light'
  const stored = window.localStorage.getItem(MODE_KEY)
  if (stored === 'dark' || stored === 'light') return stored
  if (window.matchMedia?.('(prefers-color-scheme: dark)').matches) return 'dark'
  return 'light'
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  // Lazy init reads the actual mode immediately on the client; SSR falls back
  // to 'light'. Without lazy init, the first render uses 'light' and the
  // [mode] effect writes data-mode='light' to <html> and localStorage BEFORE
  // a separate effect rereads localStorage — clobbering a saved 'dark' value
  // and causing a dark→light→dark flash on every page load.
  const [mode, setModeState] = useState<Mode>(() => readInitialMode())

  useEffect(() => {
    const root = document.documentElement
    root.setAttribute('data-theme', APP_THEME)
    root.setAttribute('data-mode', mode)
    try { window.localStorage.setItem(THEME_KEY, APP_THEME) } catch {}
    try { window.localStorage.setItem(MODE_KEY, mode) } catch {}
  }, [mode])

  // Honor OS preference live (only if user hasn't explicitly chosen a mode).
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (e: MediaQueryListEvent) => {
      const explicit = window.localStorage.getItem(MODE_KEY)
      if (explicit !== 'light' && explicit !== 'dark') {
        setModeState(e.matches ? 'dark' : 'light')
      }
    }
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  const toggleTheme = useCallback(() => {
    setModeState((m) => (m === 'dark' ? 'light' : 'dark'))
  }, [])

  const setMode = useCallback((m: Mode) => setModeState(m), [])

  return (
    <ThemeContext.Provider value={{ mode, theme: mode, toggleTheme, setMode }}>
      {children}
    </ThemeContext.Provider>
  )
}

export function useTheme() {
  return useContext(ThemeContext)
}

/**
 * Inline FOUC-prevention script. Inject in <head> before any UI renders so
 * that the [data-theme]/[data-mode] attributes match localStorage and the
 * page paints in the correct mode.
 */
export const themeInitScript = `
(function () {
  try {
    var t = localStorage.getItem('${THEME_KEY}') || '${APP_THEME}';
    var m = localStorage.getItem('${MODE_KEY}');
    if (m !== 'light' && m !== 'dark') {
      m = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    var html = document.documentElement;
    html.setAttribute('data-theme', t);
    html.setAttribute('data-mode', m);
  } catch (e) {}
})();
`.trim()
