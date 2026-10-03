'use client'

import dynamic from 'next/dynamic'
import { DocsViewer, DocsLoadingFallback } from '../components/docs-viewer'
import { useTranslation } from '@/i18n'

const SwaggerUiWrapper = dynamic(
  () => import('@/components/swagger-ui-wrapper'),
  {
    ssr: false,
    loading: () => <DocsLoadingFallback />,
  },
)

import { BASE_API_URL as API_URL } from '@/lib/constants'

export default function SwaggerPage() {
  const { t, i18n } = useTranslation()
  const lang = i18n.language ?? 'en'
  const specUrl = `${API_URL}/docs/openapi.json?lang=${lang}`

  return (
    <DocsViewer title={t('apiDocs.swaggerTitle')}>
      <SwaggerUiWrapper specUrl={specUrl} />
    </DocsViewer>
  )
}
