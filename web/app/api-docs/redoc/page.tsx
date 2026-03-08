'use client'

import dynamic from 'next/dynamic'
import Link from 'next/link'
import { ChevronLeft } from 'lucide-react'
import { useTranslation } from '@/i18n'

const RedocWrapper = dynamic(
  () => import('@/components/redoc-wrapper'),
  {
    ssr: false,
    loading: () => (
      <div className="flex items-center justify-center h-64 text-muted-foreground text-sm animate-pulse">
        Loading…
      </div>
    ),
  },
)

import { BASE_API_URL as API_URL } from '@/lib/constants'

export default function RedocPage() {
  const { t, i18n } = useTranslation()
  const lang = i18n.language ?? 'en'
  const specUrl = `${API_URL}/docs/openapi.json?lang=${lang}`

  const labels = {
    enum:            t('apiDocs.redocEnum'),
    default:         t('apiDocs.redocDefault'),
    example:         t('apiDocs.redocExample'),
    download:        t('apiDocs.redocDownload'),
    noResultsFound:  t('apiDocs.redocNoResults'),
    responses:       t('apiDocs.redocResponses'),
    requestSamples:  t('apiDocs.redocRequestSamples'),
    responseSamples: t('apiDocs.redocResponseSamples'),
  }

  return (
    <div className="flex flex-col min-h-0">
      {/* Back nav */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-border bg-card text-sm">
        <Link
          href="/api-docs"
          className="flex items-center gap-1 text-muted-foreground hover:text-foreground transition-colors"
        >
          <ChevronLeft className="h-4 w-4" />
          {t('apiDocs.backToDocs')}
        </Link>
        <span className="text-border mx-1">/</span>
        <span className="font-medium">ReDoc</span>
      </div>

      {/* Viewer — fills remaining space */}
      <div className="flex-1 overflow-auto">
        <RedocWrapper specUrl={specUrl} labels={labels} />
      </div>
    </div>
  )
}
