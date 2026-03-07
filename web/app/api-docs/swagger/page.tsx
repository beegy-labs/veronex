'use client'

import dynamic from 'next/dynamic'
import Link from 'next/link'
import { ChevronLeft } from 'lucide-react'
import { useTranslation } from '@/i18n'

const SwaggerUiWrapper = dynamic(
  () => import('@/components/swagger-ui-wrapper'),
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

export default function SwaggerPage() {
  const { t, i18n } = useTranslation()
  const lang = i18n.language ?? 'en'
  const specUrl = `${API_URL}/docs/openapi.json?lang=${lang}`

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
        <span className="font-medium">Swagger UI</span>
      </div>

      {/* Viewer — fills remaining space */}
      <div className="flex-1 overflow-auto">
        <SwaggerUiWrapper specUrl={specUrl} />
      </div>
    </div>
  )
}
