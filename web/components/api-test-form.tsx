'use client'

import { useRef } from 'react'
import { Send, ImagePlus, X, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import type { ProviderOption } from '@/components/api-test-types'

const MAX_IMAGES = 4
const MAX_FILE_BYTES = 10 * 1024 * 1024  // 10MB pre-compress UX limit

interface ApiTestFormProps {
  providerType: string
  model: string
  prompt: string
  images: string[]          // raw base64 (no data URL prefix)
  isCompressing: boolean
  availableOptions: ProviderOption[]
  availableModels: string[]
  isGeminiProvider: boolean
  isAnyStreaming: boolean
  canRun: boolean
  authUsername: string | null
  onProviderChange: (v: string) => void
  onModelChange: (v: string) => void
  onPromptChange: (v: string) => void
  onImagesChange: (imgs: string[]) => void
  onImageAdd: (files: FileList) => void
  onImageRemove: (index: number) => void
  onRun: () => void
}

export function ApiTestForm({
  providerType, model, prompt,
  images, isCompressing,
  availableOptions, availableModels, isGeminiProvider,
  isAnyStreaming, canRun, authUsername,
  onProviderChange, onModelChange, onPromptChange,
  onImageAdd, onImageRemove, onRun,
}: ApiTestFormProps) {
  const { t } = useTranslation()
  const fileInputRef = useRef<HTMLInputElement>(null)

  function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    if (e.target.files && e.target.files.length > 0) {
      onImageAdd(e.target.files)
    }
    // Reset so the same file can be re-selected
    e.target.value = ''
  }

  const canAddMore = images.length < MAX_IMAGES && !isGeminiProvider

  return (
    <form onSubmit={(e) => { e.preventDefault(); onRun() }} className="space-y-4 pb-4">
      {/* Provider + Model */}
      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <Label>{t('test.provider')}</Label>
          <Select
            value={providerType}
            onValueChange={(v) => { onProviderChange(v); onModelChange('') }}
            disabled={isAnyStreaming}
          >
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              {availableOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label>{t('test.model')}</Label>
          <Select
            value={model}
            onValueChange={onModelChange}
            disabled={isAnyStreaming || availableModels.length === 0}
          >
            <SelectTrigger>
              <SelectValue placeholder={
                availableModels.length === 0
                  ? (isGeminiProvider ? t('test.geminiModelEmpty') : t('test.ollamaTestNoModels'))
                  : t('test.modelSelect')
              } />
            </SelectTrigger>
            <SelectContent>
              {availableModels.map((m) => (
                <SelectItem key={m} value={m}>{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Prompt + Image button + Run button */}
      <div className="flex gap-3 items-end">
        <div className="flex-1 space-y-1.5">
          <Label>{t('test.prompt')}</Label>
          <textarea
            value={prompt}
            onChange={(e) => onPromptChange(e.target.value)}
            rows={3}
            placeholder={t('test.promptPlaceholder')}
            className="flex min-h-[72px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 resize-y"
          />
        </div>

        <div className="flex flex-col gap-2 mb-0.5">
          {/* Image attach button — Ollama only, hidden for Gemini */}
          {!isGeminiProvider && (
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
                variant="outline"
                size="icon"
                disabled={!canAddMore || isAnyStreaming || isCompressing}
                title={t('test.imageAttach')}
                onClick={() => fileInputRef.current?.click()}
              >
                {isCompressing
                  ? <Loader2 className="h-4 w-4 animate-spin" />
                  : <ImagePlus className="h-4 w-4" />
                }
              </Button>
            </>
          )}

          <Button
            type="submit"
            disabled={!canRun}
            className="shrink-0"
          >
            <Send className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Image thumbnails */}
      {images.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {images.map((b64, i) => (
            <div key={i} className="relative group">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`data:image/jpeg;base64,${b64}`}
                alt={`image-${i + 1}`}
                className="h-16 w-16 rounded-md object-cover border border-border"
              />
              <button
                type="button"
                onClick={() => onImageRemove(i)}
                className="absolute -top-1.5 -right-1.5 hidden group-hover:flex h-4 w-4 items-center justify-center rounded-full bg-destructive text-destructive-foreground"
                title={t('test.imageRemove')}
              >
                <X className="h-2.5 w-2.5" />
              </button>
            </div>
          ))}
          {isCompressing && (
            <div className="flex h-16 w-16 items-center justify-center rounded-md border border-dashed border-border">
              <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
            </div>
          )}
        </div>
      )}

      {/* Auth indicator */}
      {authUsername && (
        <p className="text-xs text-muted-foreground">
          {t('test.runningAs')}: <span className="font-medium text-foreground">{authUsername}</span>
        </p>
      )}
    </form>
  )
}
