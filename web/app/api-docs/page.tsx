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
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold">{t('apiDocs.title')}</h1>
        <p className="text-muted-foreground text-sm mt-1">{t('apiDocs.description')}</p>
      </div>

      {/* Viewer cards — internal navigation */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        {docs.map(({ titleKey, descKey, href, icon: Icon, badge }) => (
          <Card key={badge} className="flex flex-col">
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <Icon className="h-5 w-5 text-primary shrink-0" />
                <CardTitle className="text-base">{t(titleKey)}</CardTitle>
                <span className="ml-auto text-xs bg-muted text-muted-foreground rounded px-1.5 py-0.5 font-mono">
                  {badge}
                </span>
              </div>
            </CardHeader>
            <CardContent className="flex-1 flex flex-col gap-4">
              <p className="text-sm text-muted-foreground">{t(descKey)}</p>
              <Button asChild variant="outline" size="sm" className="mt-auto w-fit">
                <Link href={href}>
                  {t('apiDocs.openDocs')}
                  <ArrowRight className="ml-1.5 h-3.5 w-3.5" />
                </Link>
              </Button>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Raw spec link */}
      <div className="flex items-center gap-2 text-sm text-muted-foreground pt-2 border-t border-border">
        <FileJson className="h-4 w-4 shrink-0" />
        <span>{t('apiDocs.specLabel')}</span>
        <a
          href={`${API_URL}/docs/openapi.json`}
          target="_blank"
          rel="noopener noreferrer"
          className="font-mono text-xs text-primary hover:underline underline-offset-4"
        >
          {API_URL}/docs/openapi.json
        </a>
      </div>
    </div>
  )
}
