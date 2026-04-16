'use client'

import { memo } from 'react'
import { useImageDrop } from '@/hooks/use-image-drop'
import { ImageAttachButton } from './image-attach-button'
import { Send, Square, X, Loader2, AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { fmtCompact } from '@/lib/chart-theme'
import type { ProviderOption, Endpoint, TestMode } from './api-test-types'
import { useLabSettings } from '@/components/lab-settings-provider'
import type { LabSettings } from '@/lib/types'

function heuristicContextWindow(modelName: string): number | null {
  const paramMatch = modelName.match(/[:\-_](\d+\.?\d*)b/i)
  if (!paramMatch) return null
  const b = parseFloat(paramMatch[1])
  if (b <= 2) return 4_096
  if (b <= 6) return 32_768
  if (b <= 13) return 32_768
  return 131_072
}

function getMultiturnWarnings(
  modelName: string,
  lab: LabSettings,
  conversationTokens?: number,
  modelContextWindows?: Record<string, number>,
): string[] {
  const warnings: string[] = []
  const paramMatch = modelName.match(/[:\-_](\d+\.?\d*)b/i)
  if (paramMatch) {
    const params = parseFloat(paramMatch[1])
    if (params < lab.multiturn_min_params) {
      warnings.push(`model_too_small:${params}:${lab.multiturn_min_params}`)
    }
  }
  if (lab.multiturn_allowed_models.length > 0 && !lab.multiturn_allowed_models.includes(modelName)) {
    warnings.push('model_not_allowed')
  }
  if (conversationTokens && conversationTokens > 0) {
    const ctxWindow = modelContextWindows?.[modelName] ?? heuristicContextWindow(modelName)
    if (ctxWindow !== null && ctxWindow > 0 && conversationTokens > ctxWindow * 0.85) {
      warnings.push(`context_too_large:${conversationTokens}:${ctxWindow}`)
    }
  }
  return warnings
}

interface ApiTestFormProps {
  mode: TestMode
  providerType: string
  model: string
  prompt: string
  images: string[]          // raw base64 (no data URL prefix)
  maxImages: number         // from lab_settings.max_images_per_request
  isCompressing: boolean
  conversationTokenEstimate?: number
  modelContextWindows?: Record<string, number>
  availableOptions: ProviderOption[]
  availableModels: string[]
  isGeminiProvider: boolean
  canRun: boolean
  authUsername: string | null
  endpoint: Endpoint
  useApiKey: boolean
  apiKeyValue: string
  onModeChange: (v: TestMode) => void
  onProviderChange: (v: string) => void
  onModelChange: (v: string) => void
  onPromptChange: (v: string) => void
  onImageAdd: (files: FileList) => void
  onImageRemove: (index: number) => void
  onEndpointChange: (v: Endpoint) => void
  onUseApiKeyChange: (v: boolean) => void
  onApiKeyValueChange: (v: string) => void
  isStreaming: boolean
  onRun: () => void
  onStop: () => void
}

export const ApiTestForm = memo(function ApiTestForm({
  mode, providerType, model, prompt,
  images, maxImages, isCompressing, conversationTokenEstimate, modelContextWindows,
  availableOptions, availableModels, isGeminiProvider,
  canRun, authUsername,
  endpoint, useApiKey, apiKeyValue,
  onModeChange, onProviderChange, onModelChange, onPromptChange,
  onImageAdd, onImageRemove,
  onEndpointChange, onUseApiKeyChange, onApiKeyValueChange,
  isStreaming, onRun, onStop,
}: ApiTestFormProps) {
  const { t } = useTranslation()
  const { labSettings } = useLabSettings()

  const multiturnWarnings = (mode === 'conversation' && model && labSettings)
    ? getMultiturnWarnings(model, labSettings, conversationTokenEstimate, modelContextWindows)
    : []

  const canAddMore = images.length < maxImages && !isGeminiProvider && maxImages > 0
  const { isDragging, handleDragOver, handleDragLeave, handleDrop } = useImageDrop(canAddMore, onImageAdd)

  return (
    <form
      onSubmit={(e) => { e.preventDefault(); onRun() }}
      className="space-y-4 pb-4"
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Mode toggle */}
      <div className="flex items-center gap-1 p-0.5 rounded-md bg-muted w-fit">
        {(['single', 'conversation'] as TestMode[]).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => onModeChange(m)}
            className={`px-3 py-1 text-xs font-medium rounded transition-colors ${
              mode === m
                ? 'bg-background text-foreground shadow-sm'
                : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            {t(m === 'single' ? 'test.modeSingle' : 'test.modeConversation')}
          </button>
        ))}
      </div>

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
                <SelectItem key={m} value={m} className="text-xs font-mono">{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          {multiturnWarnings.length > 0 && (
            <div className="space-y-1 pt-0.5">
              {multiturnWarnings.map((w) => {
                let msg: string
                if (w.startsWith('model_too_small:')) {
                  const [, params, min] = w.split(':')
                  msg = t('common.multiturnWarnTooSmall', { params, min })
                } else if (w === 'model_not_allowed') {
                  msg = t('common.multiturnWarnNotAllowed')
                } else if (w.startsWith('context_too_large:')) {
                  const [, tokens, ctx] = w.split(':')
                  msg = t('common.multiturnWarnContextTooLarge', { tokens: fmtCompact(Number(tokens)), ctx: fmtCompact(Number(ctx)) })
                } else {
                  msg = w
                }
                return (
                  <div key={w} className="flex items-start gap-1.5 text-[11px] text-status-warning-fg">
                    <AlertTriangle className="h-3 w-3 shrink-0 mt-0.5" />
                    <span>{msg}</span>
                  </div>
                )
              })}
            </div>
          )}
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
            <SelectItem value="/v1/chat/completions" className="text-xs font-mono">/v1/chat/completions</SelectItem>
            {!isGeminiProvider && (
              <>
                <SelectItem value="/api/chat" className="text-xs font-mono">/api/chat</SelectItem>
                <SelectItem value="/api/generate" className="text-xs font-mono">/api/generate</SelectItem>
              </>
            )}
            {isGeminiProvider && (
              <SelectItem value="/v1beta/models">{t('test.endpointGemini')}</SelectItem>
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

      {/* Prompt + Image button + Run button — hidden in conversation mode (input moves to chat area) */}
      {mode !== 'conversation' && (
        <div className={`border border-border rounded-md${isDragging ? ' ring-2 ring-ring ring-offset-2' : ''}`}>
          {/* Image thumbnails */}
          {images.length > 0 && (
            <div className="px-4 pt-3 flex flex-wrap gap-2">
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

          {/* Textarea */}
          <div className="px-4 pt-3 pb-0">
            <textarea
              id="test-prompt"
              value={prompt}
              onChange={(e) => onPromptChange(e.target.value)}
              rows={3}
              placeholder={t('test.promptPlaceholder')}
              className="w-full border-0 bg-transparent px-0 py-1 text-sm placeholder:text-muted-foreground focus-visible:outline-none resize-y"
            />
          </div>

          {/* Bottom toolbar */}
          <div className="px-4 pb-3 flex items-center gap-2 pt-2 border-t border-border/50">
            {isStreaming ? (
              <Button
                type="button"
                variant="destructive"
                className="rounded-full px-5 h-8 text-sm font-medium"
                onClick={onStop}
              >
                <Square className="h-3 w-3 mr-1.5" />
                {t('test.stop')}
              </Button>
            ) : (
              <Button
                type="submit"
                disabled={!canRun}
                className="rounded-full px-5 h-8 text-sm font-medium"
                aria-label={t('test.run')}
              >
                <Send className="h-3.5 w-3.5 mr-1.5" />
                {t('test.run')}
              </Button>
            )}
            {!isGeminiProvider && (
              <ImageAttachButton
                canAddMore={canAddMore}
                isCompressing={isCompressing}
                onImageAdd={onImageAdd}
              />
            )}
          </div>
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
})
