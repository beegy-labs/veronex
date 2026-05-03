'use client'

import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { COPY_FEEDBACK_MS } from '@/lib/constants'
import { useTranslation } from '@/i18n'

export function CopyButton({ text, className = 'vds-h-7 vds-w-7' }: { text: string; className?: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  async function handleCopy() {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), COPY_FEEDBACK_MS)
  }

  return (
    <Button variant="ghost" size="icon" className={className} aria-label={copied ? t('common.copied') : t('common.copy')} onClick={handleCopy} title={t('common.copy')}>
      {copied ? <Check className="vds-h-3.5 vds-w-3.5 vds-text-success" /> : <Copy className="vds-h-3.5 vds-w-3.5" />}
    </Button>
  )
}
