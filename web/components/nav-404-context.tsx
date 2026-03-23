'use client'

import { createContext, useContext, useState, useCallback, ReactNode } from 'react'

interface Nav404Context {
  hidden: Set<string>
  hideSection: (section: string) => void
}

const Nav404Context = createContext<Nav404Context>({
  hidden: new Set(),
  hideSection: () => {},
})

export function Nav404Provider({ children }: { children: ReactNode }) {
  const [hidden, setHidden] = useState<Set<string>>(new Set())

  const hideSection = useCallback((section: string) => {
    setHidden(prev => {
      if (prev.has(section)) return prev
      const next = new Set(prev)
      next.add(section)
      return next
    })
  }, [])

  return (
    <Nav404Context.Provider value={{ hidden, hideSection }}>
      {children}
    </Nav404Context.Provider>
  )
}

/** Returns true if the section should be hidden due to a 404 response. */
export function useNav404() {
  return useContext(Nav404Context)
}

/**
 * Call this in a component when an API call returns 404 to hide the
 * corresponding nav section. Safe to call multiple times — idempotent.
 *
 * @example
 * const { data, error } = useQuery(mcpServersQuery())
 * useHideOnNotFound('mcp', error?.status === 404)
 */
export function useHideOnNotFound(section: string, is404: boolean) {
  const { hideSection } = useNav404()
  if (is404) hideSection(section)
}
