'use client'

import Link from 'next/link'
import { FileJson, BookOpen, Layers, ArrowRight } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { BASE_API_URL as API_URL } from '@/lib/constants'

// ── Page ───────────────────────────────────────────────────────────────────────

export default function ApiDocsPage() {
  usePageGuard('dashboard_view')
  const { t } = useTranslation()

  const docs = [
    {
      titleKey: 'apiDocs.swaggerTitle',
      descKey:  'apiDocs.swaggerDesc',
      href:     '/api-docs/swagger',
      icon:     BookOpen,
      badge:    'Swagger UI',
    },
    {
      titleKey: 'apiDocs.redocTitle',
      descKey:  'apiDocs.redocDesc',
      href:     '/api-docs/redoc',
      icon:     Layers,
      badge:    'ReDoc',
    },
  ]

  return (
    <div className="vds-space-y-6">
      <div>
        <h1 className="vds-text-2xl vds-font-600">{t('apiDocs.title')}</h1>
        <p className="vds-text-dim vds-text-sm vds-mt-1">{t('apiDocs.description')}</p>
      </div>

      {/* Viewer cards — internal navigation */}
      <div className="vds-grid vds-grid-cols-1 vds-sm:grid-cols-2 vds-gap-4">
        {docs.map(({ titleKey, descKey, href, icon: Icon, badge }) => (
          <Card key={badge} className="vds-flex vds-flex-col">
            <CardHeader className="vds-pb-3">
              <div className="vds-flex vds-items-center vds-gap-2">
                <Icon className="vds-h-5 vds-w-5 vds-text-primary vds-flex-shrink-0" />
                <CardTitle className="vds-text-base">{t(titleKey)}</CardTitle>
                <span className="vds-ml-auto vds-text-xs vds-bg-muted vds-text-dim vds-rounded vds-px-1.5 vds-py-0.5 vds-font-mono">
                  {badge}
                </span>
              </div>
            </CardHeader>
            <CardContent className="vds-flex-1 vds-flex vds-flex-col vds-gap-4">
              <p className="vds-text-sm vds-text-dim">{t(descKey)}</p>
              <Button asChild variant="outline" size="sm" className="vds-mt-auto vds-w-fit">
                <Link href={href}>
                  {t('apiDocs.openDocs')}
                  <ArrowRight className="vds-ml-1.5 vds-h-3.5 vds-w-3.5" />
                </Link>
              </Button>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Raw spec link */}
      <div className="vds-flex vds-items-center vds-gap-2 vds-text-sm vds-text-dim vds-pt-2 vds-border-t-1 vds-border-subtle">
        <FileJson className="vds-h-4 vds-w-4 vds-flex-shrink-0" />
        <span>{t('apiDocs.specLabel')}</span>
        <a
          href={`${API_URL}/docs/openapi.json`}
          target="_blank"
          rel="noopener noreferrer"
          className="vds-font-mono vds-text-xs vds-text-primary vds-hover:underline vds-underline-offset-4"
        >
          {API_URL}/docs/openapi.json
        </a>
      </div>
    </div>
  )
}
