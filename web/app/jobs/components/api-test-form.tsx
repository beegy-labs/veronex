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
      className="vds-space-y-4 vds-pb-4"
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Mode toggle */}
      <div className="vds-flex vds-items-center vds-gap-1 vds-p-0.5 vds-rounded-md vds-bg-muted vds-w-fit">
        {(['single', 'conversation'] as TestMode[]).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => onModeChange(m)}
            className={`vds-px-3 vds-py-1 vds-text-xs vds-font-500 vds-rounded vds-transition-colors ${
 mode === m
 ? 'vds-bg-page vds-text-primary vds-shadow-sm'
 : 'vds-text-dim vds-hover:text-primary'
 }`}
          >
            {t(m === 'single' ? 'test.modeSingle' : 'test.modeConversation')}
          </button>
        ))}
      </div>

      {/* Provider + Model */}
      <div className="vds-grid vds-grid-cols-2 vds-gap-4">
        <div className="vds-space-y-1.5">
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
        <div className="vds-space-y-1.5">
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
                <SelectItem key={m} value={m} className="vds-text-xs vds-font-mono">{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          {multiturnWarnings.length > 0 && (
            <div className="vds-space-y-1 vds-pt-0.5">
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
                  <div key={w} className="vds-flex vds-items-start vds-gap-1.5 vds-text-2xs vds-text-warning">
                    <AlertTriangle className="vds-h-3 vds-w-3 vds-flex-shrink-0 vds-mt-0.5" />
                    <span>{msg}</span>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </div>

      {/* Endpoint selector */}
      <div className="vds-space-y-1.5">
        <Label htmlFor="test-endpoint">{t('test.endpoint')}</Label>
        <Select
          value={endpoint}
          onValueChange={(v) => onEndpointChange(v as Endpoint)}
        >
          <SelectTrigger id="test-endpoint" aria-label={t('test.endpoint')}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="/v1/chat/completions" className="vds-text-xs vds-font-mono">/v1/chat/completions</SelectItem>
            {!isGeminiProvider && (
              <>
                <SelectItem value="/api/chat" className="vds-text-xs vds-font-mono">/api/chat</SelectItem>
                <SelectItem value="/api/generate" className="vds-text-xs vds-font-mono">/api/generate</SelectItem>
              </>
            )}
            {isGeminiProvider && (
              <SelectItem value="/v1beta/models">{t('test.endpointGemini')}</SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>

      {/* API Key toggle + input */}
      <div className="vds-space-y-2">
        <div className="vds-flex vds-items-center vds-gap-3">
          <Switch
            id="test-use-api-key"
            checked={useApiKey}
            onCheckedChange={onUseApiKeyChange}
          />
          <Label htmlFor="test-use-api-key" className="vds-cursor-pointer">{t('test.apiKeyToggle')}</Label>
          {!useApiKey && (
            <span className="vds-text-xs vds-text-dim">{t('test.noApiKey')}</span>
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
        <div className={`vds-border-1 vds-border-subtle vds-rounded-md${isDragging ? ' vds-ring-2 vds-ring-focus vds-ring-offset-2' : ''}`}>
          {/* Image thumbnails */}
          {images.length > 0 && (
            <div className="vds-px-4 vds-pt-3 vds-flex vds-flex-wrap vds-gap-2">
              {images.map((b64, i) => (
                <div key={b64.slice(0, 16)} className="vds-relative vds-group">
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img
                    src={`data:image/jpeg;base64,${b64}`}
                    alt={`image-${i + 1}`}
                    className="vds-h-12 vds-w-12 vds-sm:h-16 vds-sm:w-16 vds-rounded-md vds-object-cover vds-border-1 vds-border-subtle"
                  />
                  <button
                    type="button"
                    onClick={() => onImageRemove(i)}
                    aria-label={t('test.imageRemove')}
                    className="vds-absolute vds--top-1.5 vds--right-1.5 vds-hidden vds-group-hover:flex vds-h-4 vds-w-4 vds-items-center vds-justify-center vds-rounded-full vds-bg-destructive vds-text-destructive-fg"
                    title={t('test.imageRemove')}
                  >
                    <X className="vds-h-2.5 vds-w-2.5" />
                  </button>
                </div>
              ))}
              {isCompressing && (
                <div className="vds-flex vds-h-12 vds-w-12 vds-sm:h-16 vds-sm:w-16 vds-items-center vds-justify-center vds-rounded-md vds-border-1 vds-border-dashed vds-border-subtle">
                  <Loader2 className="vds-h-5 vds-w-5 vds-animate-spin vds-text-dim" aria-label={t('test.imageCompressing')} />
                </div>
              )}
            </div>
          )}

          {/* Textarea */}
          <div className="vds-px-4 vds-pt-3 vds-pb-0">
            <textarea
              id="test-prompt"
              value={prompt}
              onChange={(e) => onPromptChange(e.target.value)}
              rows={3}
              placeholder={t('test.promptPlaceholder')}
              className="vds-w-full vds-border-0 vds-bg-transparent vds-px-0 vds-py-1 vds-text-sm vds-placeholder:text-dim vds-focus-visible:outline-none"
            />
          </div>

          {/* Bottom toolbar */}
          <div className="vds-px-4 vds-pb-3 vds-flex vds-items-center vds-gap-2 vds-pt-2 vds-border-t-1 vds-border-subtle/50">
            {isStreaming ? (
              <Button
                type="button"
                variant="destructive"
                className="vds-rounded-full vds-px-5 vds-h-8 vds-text-sm vds-font-500"
                onClick={onStop}
              >
                <Square className="vds-h-3 vds-w-3 vds-mr-1.5" />
                {t('test.stop')}
              </Button>
            ) : (
              <Button
                type="submit"
                disabled={!canRun}
                className="vds-rounded-full vds-px-5 vds-h-8 vds-text-sm vds-font-500"
                aria-label={t('test.run')}
              >
                <Send className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
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
        <p className="vds-text-xs vds-text-dim">
          {t('test.runningAs')}: <span className="vds-font-500 vds-text-primary">{authUsername}</span>
        </p>
      )}
    </form>
  )
})
