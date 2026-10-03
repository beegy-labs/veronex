'use client'

import dynamic from 'next/dynamic'
import { DocsViewer, DocsLoadingFallback } from '../components/docs-viewer'
import { useTranslation } from '@/i18n'

const RedocWrapper = dynamic(
  () => import('@/components/redoc-wrapper'),
  {
    ssr: false,
    loading: () => <DocsLoadingFallback />,
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
    <DocsViewer title={t('apiDocs.redocTitle')}>
      <RedocWrapper specUrl={specUrl} labels={labels} />
    </DocsViewer>
  )
}
