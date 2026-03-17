'use client'

import { RedocStandalone } from 'redoc'

interface RedocWrapperProps {
  specUrl: string
  labels: {
    enum: string
    default: string
    example: string
    download: string
    noResultsFound: string
    responses: string
    requestSamples: string
    responseSamples: string
  }
}

/**
 * Thin wrapper around RedocStandalone.
 * Loaded via dynamic() with ssr:false from the /api-docs/redoc page.
 *
 * NOTE: Redoc does not support CSS custom properties — all theme values must be
 * static hex strings. These are intentional light-mode palette constants;
 * the `no-hardcoded-hex` linting exception applies to this file only.
 */

// ── Veronex light-mode palette (Redoc-only) ──────────────────────────────────
const REDOC_COLOR_PRIMARY        = '#0f3325'
const REDOC_COLOR_SUCCESS        = '#16a34a'
const REDOC_COLOR_WARNING        = '#d97706'
const REDOC_COLOR_ERROR          = '#dc2626'
const REDOC_COLOR_INFO           = '#2563eb'
const REDOC_TEXT_PRIMARY         = '#141a14'
const REDOC_TEXT_SECONDARY       = '#334155'
const REDOC_BORDER_DARK          = '#cbd5e1'
const REDOC_BORDER_LIGHT         = '#e2e8e0'
const REDOC_SIDEBAR_BG           = '#f2f4f2'
const REDOC_SIDEBAR_ACTIVE_BG    = '#e2e8e0'
const REDOC_RIGHT_PANEL_BG       = '#1a2118'
const REDOC_CODE_BLOCK_BG        = '#111412'
const REDOC_RESP_SUCCESS_BG      = '#f0fdf4'
const REDOC_RESP_SUCCESS_TAB     = '#14532d'
const REDOC_RESP_ERROR_BG        = '#fef2f2'
const REDOC_RESP_ERROR_TAB       = '#7f1d1d'
const REDOC_RESP_REDIRECT_BG     = '#fffbeb'
const REDOC_RESP_REDIRECT_TAB    = '#78350f'
const REDOC_RESP_INFO_BG         = '#eff6ff'
const REDOC_RESP_INFO_TAB        = '#1e3a5f'

export default function RedocWrapper({ specUrl, labels }: RedocWrapperProps) {
  return (
    <RedocStandalone
      specUrl={specUrl}
      options={{
        nativeScrollbars: false,
        disableSearch: false,
        expandResponses: '200,201',
        hideDownloadButton: false,
        labels,
        theme: {
          spacing: { unit: 5 },
          colors: {
            primary: { main: REDOC_COLOR_PRIMARY },
            success: { main: REDOC_COLOR_SUCCESS },
            warning: { main: REDOC_COLOR_WARNING },
            error:   { main: REDOC_COLOR_ERROR },
            text: {
              primary:   REDOC_TEXT_PRIMARY,
              secondary: REDOC_TEXT_SECONDARY,
            },
            border: { dark: REDOC_BORDER_DARK, light: REDOC_BORDER_LIGHT },
            responses: {
              success:  { color: REDOC_COLOR_SUCCESS,  backgroundColor: REDOC_RESP_SUCCESS_BG,  tabTextColor: REDOC_RESP_SUCCESS_TAB  },
              error:    { color: REDOC_COLOR_ERROR,    backgroundColor: REDOC_RESP_ERROR_BG,    tabTextColor: REDOC_RESP_ERROR_TAB    },
              redirect: { color: REDOC_COLOR_WARNING,  backgroundColor: REDOC_RESP_REDIRECT_BG, tabTextColor: REDOC_RESP_REDIRECT_TAB },
              info:     { color: REDOC_COLOR_INFO,     backgroundColor: REDOC_RESP_INFO_BG,     tabTextColor: REDOC_RESP_INFO_TAB     },
            },
          },
          typography: {
            fontSize: '14px',
            fontFamily: 'ui-sans-serif, system-ui, sans-serif',
            headings: { fontFamily: 'ui-sans-serif, system-ui, sans-serif' },
            code: { fontSize: '13px', fontFamily: 'ui-monospace, SFMono-Regular, monospace' },
          },
          sidebar: {
            width: '240px',
            backgroundColor: REDOC_SIDEBAR_BG,
            textColor:       REDOC_TEXT_PRIMARY,
            activeTextColor: REDOC_COLOR_PRIMARY,
            groupItems: { activeBackgroundColor: REDOC_SIDEBAR_ACTIVE_BG, activeTextColor: REDOC_COLOR_PRIMARY },
            level1Items:  { activeBackgroundColor: REDOC_SIDEBAR_ACTIVE_BG, activeTextColor: REDOC_COLOR_PRIMARY },
          },
          rightPanel: {
            backgroundColor: REDOC_RIGHT_PANEL_BG,
          },
          codeBlock: {
            backgroundColor: REDOC_CODE_BLOCK_BG,
          },
        },
      }}
    />
  )
}
