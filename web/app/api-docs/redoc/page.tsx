'use client'

import dynamic from 'next/dynamic'
import Link from 'next/link'
import { ChevronLeft } from 'lucide-react'
import { useTranslation } from '@/i18n'

function RedocLoadingFallback() {
  const { t } = useTranslation()
  return (
    <div className="vds-flex vds-items-center vds-justify-center vds-h-64 vds-text-dim vds-text-sm vds-animate-pulse">
      {t('apiDocs.loading')}
    </div>
  )
}

const RedocWrapper = dynamic(
  () => import('@/components/redoc-wrapper'),
  {
    ssr: false,
    loading: () => <RedocLoadingFallback />,
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
    <div className="vds-flex vds-flex-col vds-min-h-0">
      {/* Back nav */}
      <div className="vds-flex vds-items-center vds-gap-2 vds-px-4 vds-py-2 vds-border-b-1 vds-border-subtle vds-bg-card vds-text-sm">
        <Link
          href="/api-docs"
          className="vds-flex vds-items-center vds-gap-1 vds-text-dim vds-hover:text-primary vds-transition-colors"
        >
          <ChevronLeft className="vds-h-4 vds-w-4" />
          {t('apiDocs.backToDocs')}
        </Link>
        <span className="vds-text-dim vds-mx-1">/</span>
        <span className="vds-font-500">{t('apiDocs.redocTitle')}</span>
      </div>

      {/* Viewer — fills remaining space */}
      <div className="vds-flex-1 vds-overflow-auto">
        <RedocWrapper specUrl={specUrl} labels={labels} />
      </div>
    </div>
  )
}
