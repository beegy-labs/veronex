'use client'

/**
 * LabSettingsProvider — SSOT for experimental feature flags
 *
 * Fetches GET /v1/dashboard/lab once on mount and provides the result
 * to the entire component tree via React context.  All components that
 * need to gate Gemini-related UI must consume this context via
 * `useLabSettings()` — never read lab settings in local component state.
 *
 * Defaults:
 *   - `labSettings === null`  → still loading (render nothing / skeleton)
 *   - fetch error / 401       → defaults to { gemini_function_calling: false }
 *     (fail-safe: hide experimental features when auth is unclear)
 */

import { createContext, useContext, useState, useEffect, useCallback } from 'react'
import { api } from '@/lib/api'
import { DEFAULT_MAX_IMAGES, DEFAULT_MAX_IMAGE_B64_BYTES } from '@/lib/constants'
import type { LabSettings } from '@/lib/types'

// ── Context ────────────────────────────────────────────────────────────────────

interface LabSettingsContextValue {
  /** null while initial fetch is in-flight */
  labSettings: LabSettings | null
  /** Re-fetch from server — call after PATCH /v1/dashboard/lab */
  refetch: () => Promise<void>
}

const LabSettingsContext = createContext<LabSettingsContextValue>({
  labSettings: null,
  refetch: async () => {},
})

// ── Provider ───────────────────────────────────────────────────────────────────

export function LabSettingsProvider({ children }: { children: React.ReactNode }) {
  const [labSettings, setLabSettings] = useState<LabSettings | null>(null)

  const refetch = useCallback(async () => {
    try {
      setLabSettings(await api.labSettings())
    } catch {
      // Unauthenticated, server error, or login page — default all features off.
      // This mirrors LabSettings::default() on the Rust side.
      setLabSettings({ gemini_function_calling: false, max_images_per_request: DEFAULT_MAX_IMAGES, max_image_b64_bytes: DEFAULT_MAX_IMAGE_B64_BYTES, mcp_orchestrator_model: null, updated_at: '' })
    }
  }, [])

  useEffect(() => { refetch() }, [refetch])

  return (
    <LabSettingsContext.Provider value={{ labSettings, refetch }}>
      {children}
    </LabSettingsContext.Provider>
  )
}

// ── Hook ───────────────────────────────────────────────────────────────────────

export function useLabSettings(): LabSettingsContextValue {
  return useContext(LabSettingsContext)
}
