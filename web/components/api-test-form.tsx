'use client'

import { useRef, useState, useCallback } from 'react'
import { Send, ImagePlus, X, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import type { ProviderOption, Endpoint } from '@/components/api-test-types'

interface ApiTestFormProps {
  providerType: string
  model: string
  prompt: string
  images: string[]          // raw base64 (no data URL prefix)
  maxImages: number         // from lab_settings.max_images_per_request
  isCompressing: boolean
  availableOptions: ProviderOption[]
  availableModels: string[]
  isGeminiProvider: boolean
  canRun: boolean
  authUsername: string | null
  endpoint: Endpoint
  useApiKey: boolean
  apiKeyValue: string
  onProviderChange: (v: string) => void
  onModelChange: (v: string) => void
  onPromptChange: (v: string) => void
  onImageAdd: (files: FileList) => void
  onImageRemove: (index: number) => void
  onEndpointChange: (v: Endpoint) => void
  onUseApiKeyChange: (v: boolean) => void
  onApiKeyValueChange: (v: string) => void
  onRun: () => void
}

export function ApiTestForm({
  providerType, model, prompt,
  images, maxImages, isCompressing,
  availableOptions, availableModels, isGeminiProvider,
  canRun, authUsername,
  endpoint, useApiKey, apiKeyValue,
  onProviderChange, onModelChange, onPromptChange,
  onImageAdd, onImageRemove,
  onEndpointChange, onUseApiKeyChange, onApiKeyValueChange,
  onRun,
}: ApiTestFormProps) {
  const { t } = useTranslation()
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    if (e.target.files && e.target.files.length > 0) {
      onImageAdd(e.target.files)
    }
    // Reset so the same file can be re-selected
    e.target.value = ''
  }

  const canAddMore = images.length < maxImages && !isGeminiProvider && maxImages > 0

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    if (canAddMore) setIsDragging(true)
  }, [canAddMore])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }, [])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
    if (!canAddMore) return
    const files = e.dataTransfer.files
    if (files.length > 0) {
      const imageFiles = Array.from(files).filter((f) => f.type.startsWith('image/'))
      if (imageFiles.length > 0) {
        const dt = new DataTransfer()
        imageFiles.forEach((f) => dt.items.add(f))
        onImageAdd(dt.files)
      }
    }
  }, [canAddMore, onImageAdd])

  return (
    <form
      onSubmit={(e) => { e.preventDefault(); onRun() }}
      className={`space-y-4 pb-4 ${isDragging ? 'ring-2 ring-ring ring-offset-2 rounded-md' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Provider + Model */}
      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <Label htmlFor="test-provider">{t('test.provider')}</Label>
          <Select
            value={providerType}
            onValueChange={(v) => { onProviderChange(v); onModelChange('') }}
          >
            <SelectTrigger id="test-provider" aria-label={t('test.provider')}><SelectValue /></SelectTrigger>
            <SelectContent>
              {availableOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="test-model">{t('test.model')}</Label>
          <Select
            value={model}
            onValueChange={onModelChange}
            disabled={availableModels.length === 0}
          >
            <SelectTrigger id="test-model" aria-label={t('test.model')}>
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

      {/* Endpoint selector */}
      <div className="space-y-1.5">
        <Label htmlFor="test-endpoint">{t('test.endpoint')}</Label>
        <Select
          value={endpoint}
          onValueChange={(v) => onEndpointChange(v as Endpoint)}
        >
          <SelectTrigger id="test-endpoint" aria-label={t('test.endpoint')}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="/v1/chat/completions">/v1/chat/completions</SelectItem>
            {!isGeminiProvider && (
              <>
                <SelectItem value="/api/chat">/api/chat</SelectItem>
                <SelectItem value="/api/generate">/api/generate</SelectItem>
              </>
            )}
            {isGeminiProvider && (
              <SelectItem value="/v1beta/models">/v1beta/models (Gemini)</SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>

      {/* API Key toggle + input */}
      <div className="space-y-2">
        <div className="flex items-center gap-3">
          <Switch
            id="test-use-api-key"
            checked={useApiKey}
            onCheckedChange={onUseApiKeyChange}
          />
          <Label htmlFor="test-use-api-key" className="cursor-pointer">{t('test.apiKeyToggle')}</Label>
          {!useApiKey && (
            <span className="text-xs text-muted-foreground">{t('test.noApiKey')}</span>
          )}
        </div>
        {useApiKey && (
          <Input
            type="password"
            placeholder={t('test.apiKeyPlaceholder')}
            value={apiKeyValue}
            onChange={(e) => onApiKeyValueChange(e.target.value)}
          />
        )}
      </div>

      {/* Prompt + Image button + Run button */}
      <div className="flex gap-3 items-end">
        <div className="flex-1 space-y-1.5">
          <Label htmlFor="test-prompt">{t('test.prompt')}</Label>
          <textarea
            id="test-prompt"
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
                disabled={!canAddMore || isCompressing}
                aria-label={t('test.imageAttach')}
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
            aria-label={t('test.run')}
          >
            <Send className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Image thumbnails */}
      {images.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {images.map((b64, i) => (
            <div key={b64.slice(0, 16)} className="relative group">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`data:image/jpeg;base64,${b64}`}
                alt={`image-${i + 1}`}
                className="h-12 w-12 sm:h-16 sm:w-16 rounded-md object-cover border border-border"
              />
              <button
                type="button"
                onClick={() => onImageRemove(i)}
                aria-label={t('test.imageRemove')}
                className="absolute -top-1.5 -right-1.5 hidden group-hover:flex h-4 w-4 items-center justify-center rounded-full bg-destructive text-destructive-foreground"
                title={t('test.imageRemove')}
              >
                <X className="h-2.5 w-2.5" />
              </button>
            </div>
          ))}
          {isCompressing && (
            <div className="flex h-12 w-12 sm:h-16 sm:w-16 items-center justify-center rounded-md border border-dashed border-border">
              <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" aria-label={t('test.imageCompressing')} />
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
