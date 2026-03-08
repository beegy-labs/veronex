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
 * Theme tokens use Veronex light-mode palette (Redoc doesn't support CSS vars).
 */
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
            primary: { main: '#0f3325' },
            success: { main: '#16a34a' },
            warning: { main: '#d97706' },
            error:   { main: '#dc2626' },
            text: {
              primary:   '#141a14',
              secondary: '#334155',
            },
            border: { dark: '#cbd5e1', light: '#e2e8e0' },
            responses: {
              success:  { color: '#16a34a', backgroundColor: '#f0fdf4', tabTextColor: '#14532d' },
              error:    { color: '#dc2626', backgroundColor: '#fef2f2', tabTextColor: '#7f1d1d' },
              redirect: { color: '#d97706', backgroundColor: '#fffbeb', tabTextColor: '#78350f' },
              info:     { color: '#2563eb', backgroundColor: '#eff6ff', tabTextColor: '#1e3a5f' },
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
            backgroundColor: '#f2f4f2',
            textColor: '#141a14',
            activeTextColor: '#0f3325',
            groupItems: { activeBackgroundColor: '#e2e8e0', activeTextColor: '#0f3325' },
            level1Items:  { activeBackgroundColor: '#e2e8e0', activeTextColor: '#0f3325' },
          },
          rightPanel: {
            backgroundColor: '#1a2118',
          },
          codeBlock: {
            backgroundColor: '#111412',
          },
        },
      }}
    />
  )
}
