'use client'

import { useState, useRef, useEffect, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { Send, Loader2, X, Eye, EyeOff } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'

// ── Types ──────────────────────────────────────────────────────────────────────

interface OpenAIChunk {
  id?: string
  choices?: { delta?: { content?: string }; finish_reason?: string | null }[]
  error?: { message?: string }
}

type BackendOption = { value: string; label: string; isGemini: boolean }

// ── Page ───────────────────────────────────────────────────────────────────────

export default function ApiTestPage() {
  const { t } = useTranslation()
  const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

  const [apiKey, setApiKey] = useState(process.env.NEXT_PUBLIC_VERONEX_ADMIN_KEY ?? '')
  const [showApiKey, setShowApiKey] = useState(false)

  const [prompt, setPrompt] = useState('')
  const [model, setModel] = useState('')
  const [backend, setBackend] = useState('ollama')

  // ── Backends ─────────────────────────────────────────────────────────────────
  const { data: backends } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    staleTime: 60_000,
  })

  // Derive 3 distinct routing options from registered active backends:
  //   ollama      — Ollama backends (VRAM-aware routing)
  //   gemini-free — free-tier Gemini backends only (no paid fallback)
  //   gemini      — auto-routing: free-first, paid-fallback
  const availableOptions = useMemo((): BackendOption[] => {
    if (!backends) return [{ value: 'ollama', label: 'Ollama', isGemini: false }]
    const opts: BackendOption[] = []
    if (backends.some((b) => b.is_active && b.backend_type === 'ollama')) {
      opts.push({ value: 'ollama', label: 'Ollama', isGemini: false })
    }
    if (backends.some((b) => b.is_active && b.backend_type === 'gemini' && b.is_free_tier)) {
      opts.push({ value: 'gemini-free', label: t('test.geminiFree'), isGemini: true })
    }
    if (backends.some((b) => b.is_active && b.backend_type === 'gemini' && !b.is_free_tier)) {
      opts.push({ value: 'gemini', label: t('test.gemini'), isGemini: true })
    }
    return opts.length > 0 ? opts : [{ value: 'ollama', label: 'Ollama', isGemini: false }]
  }, [backends, t])

  const isGeminiBackend = availableOptions.find((o) => o.value === backend)?.isGemini ?? false

  // Ensure selected backend is always valid
  useEffect(() => {
    if (availableOptions.length > 0 && !availableOptions.find((o) => o.value === backend)) {
      setBackend(availableOptions[0].value)
      setModel('')
    }
  }, [availableOptions, backend])

  // ── Models: Ollama uses global pool; Gemini uses global pool ────────────────

  // Ollama: global model pool (synced via /backends → Ollama tab)
  const { data: ollamaModelsData } = useQuery({
    queryKey: ['ollama-models'],
    queryFn: () => api.ollamaModels(),
    enabled: !isGeminiBackend,
    staleTime: 30_000,
  })

  // Gemini: use global model pool (synced via /backends → Gemini tab)
  const { data: geminiModelsData } = useQuery({
    queryKey: ['gemini-models'],
    queryFn: () => api.geminiModels(),
    enabled: isGeminiBackend,
    staleTime: 5 * 60_000,
  })

  // Gemini: fetch policies to filter free-tier models
  const { data: geminiPolicies } = useQuery({
    queryKey: ['gemini-policies'],
    queryFn: () => api.geminiPolicies(),
    enabled: isGeminiBackend,
    staleTime: 5 * 60_000,
  })

  const availableModels = useMemo(() => {
    if (!isGeminiBackend) return ollamaModelsData?.models.map((m) => m.model_name) ?? []

    const allModels = geminiModelsData?.models.map((m) => m.model_name) ?? []
    if (backend !== 'gemini-free') return allModels

    // gemini-free: only show models with an EXPLICIT policy where available_on_free_tier=true
    // The global '*' fallback is for rate limits only — models without explicit policy
    // are treated as NOT free-tier available (conservative default)
    const policyMap = new Map(
      (geminiPolicies ?? [])
        .filter((p) => p.model_name !== '*')
        .map((p) => [p.model_name, p])
    )

    return allModels.filter((name) => policyMap.get(name)?.available_on_free_tier === true)
  }, [isGeminiBackend, backend, geminiModelsData, geminiPolicies, ollamaModelsData?.models])

  // Reset model when backend type or model list changes
  useEffect(() => {
    if (availableModels.length > 0 && !availableModels.includes(model)) {
      setModel(availableModels[0])
    }
  }, [availableModels, model])

  // ── Inference state ───────────────────────────────────────────────────────────
  const [tokens, setTokens] = useState<string[]>([])
  const [status, setStatus] = useState<'idle' | 'streaming' | 'done' | 'error'>('idle')
  const [errorMsg, setErrorMsg] = useState('')
  const readerRef = useRef<ReadableStreamDefaultReader<Uint8Array> | null>(null)

  function stopStream() {
    readerRef.current?.cancel()
    readerRef.current = null
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!prompt.trim() || status === 'streaming') return

    stopStream()
    setTokens([])
    setErrorMsg('')
    setStatus('streaming')

    try {
      const resp = await fetch(`${BASE}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-API-Key': apiKey,
        },
        body: JSON.stringify({
          model,
          messages: [{ role: 'user', content: prompt.trim() }],
          backend,
          stream: true,
        }),
      })

      if (!resp.ok || !resp.body) {
        throw new Error(`${resp.status} ${resp.statusText}`)
      }

      const reader = resp.body.getReader()
      readerRef.current = reader
      const decoder = new TextDecoder()
      let buf = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buf += decoder.decode(value, { stream: true })
        const lines = buf.split('\n')
        buf = lines.pop() ?? ''

        for (const line of lines) {
          const trimmed = line.trimEnd()
          if (!trimmed.startsWith('data:')) continue

          const raw = trimmed.slice(5)
          const data = raw.startsWith(' ') ? raw.slice(1) : raw

          if (data === '[DONE]') {
            setStatus('done')
            reader.cancel()
            return
          }

          try {
            const chunk: OpenAIChunk = JSON.parse(data)

            if (chunk.error?.message) {
              throw new Error(chunk.error.message)
            }

            const content = chunk.choices?.[0]?.delta?.content
            if (content) setTokens((prev) => [...prev, content])

            if (chunk.choices?.[0]?.finish_reason === 'stop') {
              // [DONE] follows; wait for it
            }
          } catch (parseErr) {
            if (parseErr instanceof SyntaxError) continue
            throw parseErr
          }
        }
      }

      setStatus('done')
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : t('common.unknownError'))
      setStatus('error')
    }
  }

  function handleReset() {
    stopStream()
    setTokens([])
    setErrorMsg('')
    setStatus('idle')
  }

  const output = tokens.join('')
  const isRunning = status === 'streaming'

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('test.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('test.description')}</p>
      </div>

      {/* API Key */}
      <div className="space-y-1.5">
        <Label>{t('test.apiKey')}</Label>
        <div className="relative flex items-center">
          <Input
            type={showApiKey ? 'text' : 'password'}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={t('test.apiKeyPlaceholder')}
            className="pr-10 font-mono text-sm"
          />
          <button
            type="button"
            className="absolute right-2.5 text-muted-foreground hover:text-foreground"
            onClick={() => setShowApiKey((v) => !v)}
          >
            {showApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
          </button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t('test.apiKeyHint')}
        </p>
      </div>

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Backend + Model */}
        <div className="grid grid-cols-2 gap-4">
          <div className="space-y-1.5">
            <Label>{t('test.backend')}</Label>
            <Select
              value={backend}
              onValueChange={(v) => { setBackend(v); setModel('') }}
              disabled={isRunning}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
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
              onValueChange={(v) => setModel(v)}
              disabled={isRunning || availableModels.length === 0}
            >
              <SelectTrigger>
                <SelectValue placeholder={
                  availableModels.length === 0
                    ? (isGeminiBackend ? t('test.geminiModelEmpty') : t('test.ollamaTestNoModels'))
                    : t('test.modelSelect')
                } />
              </SelectTrigger>
              <SelectContent>
                {availableModels.map((m) => (
                  <SelectItem key={m} value={m}>{m}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            {!isGeminiBackend && availableModels.length === 0 && (
              <p className="text-[11px] text-muted-foreground italic">{t('test.ollamaTestNoModels')}</p>
            )}
          </div>
        </div>

        {/* Prompt */}
        <div className="space-y-1.5">
          <Label>{t('test.prompt')}</Label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            disabled={isRunning}
            rows={4}
            placeholder={t('test.promptPlaceholder')}
            className="flex min-h-[96px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 resize-y"
          />
        </div>

        {/* Submit */}
        <div className="flex gap-3">
          <Button type="submit" disabled={!prompt.trim() || !model || !apiKey || isRunning}>
            {isRunning
              ? <><Loader2 className="h-4 w-4 animate-spin mr-2" />{t('test.streaming')}</>
              : <><Send className="h-4 w-4 mr-2" />{t('test.run')}</>}
          </Button>

          {status !== 'idle' && (
            <Button type="button" variant="outline" onClick={handleReset}>
              <X className="h-4 w-4 mr-2" />{t('test.reset')}
            </Button>
          )}
        </div>
      </form>

      {/* Output */}
      {(output || status === 'streaming' || status === 'done') && (
        <Card>
          <CardContent className="p-5">
            <div className="flex items-center justify-between mb-3">
              <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                {t('test.output')}
              </p>
              {status === 'streaming' && (
                <span className="flex items-center gap-1.5 text-xs text-status-info-fg">
                  <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse" />
                  {t('test.streaming')}
                </span>
              )}
              {status === 'done' && (
                <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30">
                  {t('test.complete')}
                </Badge>
              )}
            </div>
            <pre className="text-sm text-text-bright whitespace-pre-wrap font-mono leading-relaxed min-h-[2rem]">
              {output}
              {status === 'streaming' && (
                <span className="inline-block w-0.5 h-4 bg-muted-foreground animate-pulse ml-px align-middle" />
              )}
            </pre>
          </CardContent>
        </Card>
      )}

      {/* Error */}
      {status === 'error' && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="p-5 text-destructive">
            <p className="font-semibold">{t('test.errorTitle')}</p>
            <p className="text-sm mt-1 opacity-80">{errorMsg}</p>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
