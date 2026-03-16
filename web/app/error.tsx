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
    <div className="flex h-full min-h-screen items-center justify-center bg-background">
      <div className="mx-auto max-w-md space-y-6 p-8 text-center">
        <AlertTriangle className="mx-auto h-12 w-12 text-destructive" />
        <h1 className="text-2xl font-bold tracking-tight">{t('errorPage.title')}</h1>
        <p className="text-sm text-muted-foreground">
          {error.message || t('errorPage.fallbackMessage')}
        </p>
        <Button onClick={reset}>{t('errorPage.tryAgain')}</Button>
      </div>
    </div>
  )
}
