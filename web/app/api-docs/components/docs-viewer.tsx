'use client'

import type { ReactNode } from 'react'
import Link from 'next/link'
import { ChevronLeft } from 'lucide-react'
import { useTranslation } from '@/i18n'

export function DocsLoadingFallback() {
  const { t } = useTranslation()
  return (
    <div className="vds-flex vds-items-center vds-justify-center vds-h-64 vds-text-dim vds-text-sm vds-animate-pulse">
      {t('apiDocs.loading')}
    </div>
  )
}

export function DocsViewer({ title, children }: { title: string; children: ReactNode }) {
  const { t } = useTranslation()
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
        <span className="vds-font-500">{title}</span>
      </div>

      {/* Viewer — fills remaining space */}
      <div className="vds-flex-1 vds-overflow-auto">
        {children}
      </div>
    </div>
  )
}
