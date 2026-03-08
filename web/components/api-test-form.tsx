'use client'

import { Send } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import type { ProviderOption } from '@/components/api-test-types'

interface ApiTestFormProps {
  providerType: string
  model: string
  prompt: string
  availableOptions: ProviderOption[]
  availableModels: string[]
  isGeminiProvider: boolean
  isAnyStreaming: boolean
  canRun: boolean
  authUsername: string | null
  onProviderChange: (v: string) => void
  onModelChange: (v: string) => void
  onPromptChange: (v: string) => void
  onRun: () => void
}

export function ApiTestForm({
  providerType, model, prompt,
  availableOptions, availableModels, isGeminiProvider,
  isAnyStreaming, canRun, authUsername,
  onProviderChange, onModelChange, onPromptChange, onRun,
}: ApiTestFormProps) {
  const { t } = useTranslation()

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

      {/* Prompt + Run button */}
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
        <Button
          type="submit"
          disabled={!canRun}
          className="shrink-0 mb-0.5"
        >
          <Send className="h-4 w-4" />
        </Button>
      </div>

      {/* Auth indicator */}
      {authUsername && (
        <p className="text-xs text-muted-foreground">
          {t('test.runningAs')}: <span className="font-medium text-foreground">{authUsername}</span>
        </p>
      )}
    </form>
  )
}
