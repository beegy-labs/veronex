'use client'

import { useEffect, useState } from 'react'
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

// Thin wrapper around RedocStandalone.
// Loaded via dynamic() with ssr:false from the /api-docs/redoc page.
//
// The Redoc theme prop reads colors once at mount and stores them in JS
// state; CSS custom properties cannot reach that path. To keep the
// verodesign SSOT, every value is resolved at runtime by setting the
// vds-theme-* variable on a hidden probe element and reading the computed
// color. Redoc receives the resolved value (e.g. rgb(...)); the source of
// truth stays in app/styles/vds/theme-veronex.css.

// SSR-only sentinel — RedocStandalone is loaded with ssr:false, so this
// branch is unreachable at runtime. The CSS keyword keeps the file free
// of raw color literals.
const SSR_SENTINEL = 'transparent'

function readThemeColor(varName: string): string {
  if (typeof window === 'undefined') return SSR_SENTINEL
  const probe = document.createElement('span')
  probe.style.color = `var(${varName})`
  probe.style.display = 'none'
  document.documentElement.appendChild(probe)
  const resolved = getComputedStyle(probe).color
  probe.remove()
  return resolved || SSR_SENTINEL
}

interface ResolvedTheme {
  primary: string
  success: string
  warning: string
  error: string
  info: string
  textPrimary: string
  textSecondary: string
  borderDefault: string
  borderSubtle: string
  bgMuted: string
  bgHover: string
  bgInverse: string
  bgCode: string
  successBg: string
  errorBg: string
  warningBg: string
  infoBg: string
}

function resolveTheme(): ResolvedTheme {
  return {
    primary:        readThemeColor('--vds-theme-primary'),
    success:        readThemeColor('--vds-theme-success'),
    warning:        readThemeColor('--vds-theme-warning'),
    error:          readThemeColor('--vds-theme-error'),
    info:           readThemeColor('--vds-theme-info'),
    textPrimary:    readThemeColor('--vds-theme-text-primary'),
    textSecondary:  readThemeColor('--vds-theme-text-secondary'),
    borderDefault:  readThemeColor('--vds-theme-border-default'),
    borderSubtle:   readThemeColor('--vds-theme-border-subtle'),
    bgMuted:        readThemeColor('--vds-theme-bg-muted'),
    bgHover:        readThemeColor('--vds-theme-bg-hover'),
    bgInverse:      readThemeColor('--vds-theme-bg-inverse'),
    bgCode:         readThemeColor('--vds-theme-bg-code'),
    successBg:      readThemeColor('--vds-theme-success-bg'),
    errorBg:        readThemeColor('--vds-theme-error-bg'),
    warningBg:      readThemeColor('--vds-theme-warning-bg'),
    infoBg:         readThemeColor('--vds-theme-info-bg'),
  }
}

export default function RedocWrapper({ specUrl, labels }: RedocWrapperProps) {
  const [t, setTheme] = useState<ResolvedTheme | null>(null)

  useEffect(() => {
    setTheme(resolveTheme())
  }, [])

  if (!t) return null

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
            primary: { main: t.primary },
            success: { main: t.success },
            warning: { main: t.warning },
            error:   { main: t.error },
            text: {
              primary:   t.textPrimary,
              secondary: t.textSecondary,
            },
            border: { dark: t.borderDefault, light: t.borderSubtle },
            responses: {
              success:  { color: t.success, backgroundColor: t.successBg, tabTextColor: t.success },
              error:    { color: t.error,   backgroundColor: t.errorBg,   tabTextColor: t.error   },
              redirect: { color: t.warning, backgroundColor: t.warningBg, tabTextColor: t.warning },
              info:     { color: t.info,    backgroundColor: t.infoBg,    tabTextColor: t.info    },
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
            backgroundColor: t.bgMuted,
            textColor:       t.textPrimary,
            activeTextColor: t.primary,
            groupItems: { activeBackgroundColor: t.bgHover, activeTextColor: t.primary },
            level1Items:  { activeBackgroundColor: t.bgHover, activeTextColor: t.primary },
          },
          rightPanel: {
            backgroundColor: t.bgInverse,
          },
          codeBlock: {
            backgroundColor: t.bgCode,
          },
        },
      }}
    />
  )
}
