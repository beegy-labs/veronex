'use client'

import { useRef, useCallback } from 'react'
import { ImagePlus, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'

interface ImageAttachButtonProps {
  canAddMore: boolean
  isCompressing: boolean
  onImageAdd: (files: FileList) => void
}

export function ImageAttachButton({ canAddMore, isCompressing, onImageAdd }: ImageAttachButtonProps) {
  const { t } = useTranslation()
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleFileChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files.length > 0) onImageAdd(e.target.files)
    e.target.value = ''
  }, [onImageAdd])

  return (
    <>
      <input
        ref={fileInputRef}
        type="file"
        accept="image/*"
        multiple
        className="hidden"
        onChange={handleFileChange}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon"
        disabled={!canAddMore || isCompressing}
        aria-label={t('test.imageAttach')}
        title={t('test.imageAttach')}
        className="h-8 w-8 text-muted-foreground hover:text-foreground"
        onClick={() => fileInputRef.current?.click()}
      >
        {isCompressing
          ? <Loader2 className="h-4 w-4 animate-spin" />
          : <ImagePlus className="h-4 w-4" />
        }
      </Button>
    </>
  )
}
