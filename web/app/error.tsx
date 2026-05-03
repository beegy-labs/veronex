'use client'

import { AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'

export default function RootError({
  error,
  reset,
}: {
  error: Error & { digest?: string }
  reset: () => void
}) {
  const { t } = useTranslation()
  return (
    <div className="vds-flex vds-h-full vds-min-h-screen vds-items-center vds-justify-center vds-bg-page">
      <div className="vds-mx-auto vds-max-w-md vds-space-y-6 vds-p-8 vds-text-center">
        <AlertTriangle className="vds-mx-auto vds-h-12 vds-w-12 vds-text-destructive" />
        <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('errorPage.title')}</h1>
        <p className="vds-text-sm vds-text-dim">
          {error.message || t('errorPage.fallbackMessage')}
        </p>
        <Button onClick={reset}>{t('errorPage.tryAgain')}</Button>
      </div>
    </div>
  )
}
