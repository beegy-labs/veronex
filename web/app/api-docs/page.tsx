'use client'

import { ExternalLink, FileJson, BookOpen, Layers } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'

const API_URL = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

// ── Page ───────────────────────────────────────────────────────────────────────

export default function ApiDocsPage() {
  const { t } = useTranslation()

  const docs = [
    {
      titleKey: 'apiDocs.swaggerTitle',
      descKey: 'apiDocs.swaggerDesc',
      href: `${API_URL}/docs/swagger`,
      icon: BookOpen,
      badge: 'Swagger UI',
    },
    {
      titleKey: 'apiDocs.redocTitle',
      descKey: 'apiDocs.redocDesc',
      href: `${API_URL}/docs/redoc`,
      icon: Layers,
      badge: 'ReDoc',
    },
  ]

  return (
    <div className="p-6 space-y-6">
      <div>
        <h1 className="text-2xl font-semibold">{t('apiDocs.title')}</h1>
        <p className="text-muted-foreground text-sm mt-1">{t('apiDocs.description')}</p>
      </div>

      {/* Viewer cards */}
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
              <Button
                asChild
                variant="outline"
                size="sm"
                className="mt-auto w-fit"
              >
                <a href={href} target="_blank" rel="noopener noreferrer">
                  {t('apiDocs.openDocs')}
                  <ExternalLink className="ml-1.5 h-3.5 w-3.5" />
                </a>
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
