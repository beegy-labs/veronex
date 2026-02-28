'use client'

import { useState, useRef, useEffect, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { RetryParams } from '@/lib/types'
import { Send, Loader2, X, Eye, EyeOff, Square, RotateCcw, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { renderWithMermaid } from '@/components/mermaid-block'

// ── Types ──────────────────────────────────────────────────────────────────────

interface OpenAIChunk {
  id?: string
  choices?: { delta?: { content?: string }; finish_reason?: string | null }[]
  error?: { message?: string }
}

type BackendOption = { value: string; label: string; isGemini: boolean }
type StreamStatus = 'idle' | 'streaming' | 'done' | 'error'

interface TabMeta {
  id: number
  streamStatus: StreamStatus
}

const MAX_TABS = 10

// ── TestSession — single tab's content ─────────────────────────────────────────

interface TestSessionProps {
  hidden: boolean
  apiKey: string
  BASE: string
  tabKey: number
  retryParams?: RetryParams | null
  onRetryConsumed?: () => void
  onStatusChange: (status: StreamStatus) => void
}

function TestSession({
  hidden,
  apiKey,
  BASE,
  tabKey,
  retryParams,
  onRetryConsumed,
  onStatusChange,
}: TestSessionProps) {
  const { t } = useTranslation()

  const [prompt, setPrompt] = useState('')
  const [model, setModel] = useState('')
  const [backend, setBackend] = useState('ollama')

  const [tokens, setTokens] = useState<string[]>([])
  const [streamStatus, setStreamStatusLocal] = useState<StreamStatus>('idle')
  const [errorMsg, setErrorMsg] = useState('')
  const readerRef = useRef<ReadableStreamDefaultReader<Uint8Array> | null>(null)
  const jobIdRef = useRef<string | null>(null)
  const stoppedRef = useRef(false)

  const desiredModelRef = useRef<string | null>(null)
  const pendingSubmitRef = useRef(false)

  function setStreamStatus(s: StreamStatus) {
    setStreamStatusLocal(s)
    onStatusChange(s)
  }

  // ── Cleanup on unmount (tab close) ──────────────────────────────────────────
  useEffect(() => {
    return () => {
      stoppedRef.current = true
      readerRef.current?.cancel()
      readerRef.current = null
      if (jobIdRef.current) {
        api.cancelJob(jobIdRef.current).catch(() => {})
        jobIdRef.current = null
      }
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Retry from job modal ───────────────────────────────────────────────────
  useEffect(() => {
    if (!retryParams) return
    setPrompt(retryParams.prompt)
    setBackend(retryParams.backend)
    desiredModelRef.current = retryParams.model
    pendingSubmitRef.current = true
    onRetryConsumed?.()
  }, [retryParams]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Backends ─────────────────────────────────────────────────────────────────
  const { data: backends } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    staleTime: 60_000,
  })

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

  useEffect(() => {
    if (!backends) return
    if (!availableOptions.find((o) => o.value === backend)) {
      setBackend(availableOptions[0].value)
      setModel('')
    }
  }, [availableOptions, backend, backends])

  // ── Models ────────────────────────────────────────────────────────────────────
  const { data: ollamaModelsData } = useQuery({
    queryKey: ['ollama-models'],
    queryFn: () => api.ollamaModels(),
    enabled: !isGeminiBackend,
    staleTime: 30_000,
  })

  const { data: geminiModelsData } = useQuery({
    queryKey: ['gemini-models'],
    queryFn: () => api.geminiModels(),
    enabled: isGeminiBackend,
    staleTime: 5 * 60_000,
  })

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
    const policyMap = new Map(
      (geminiPolicies ?? []).filter((p) => p.model_name !== '*').map((p) => [p.model_name, p])
    )
    return allModels.filter((name) => policyMap.get(name)?.available_on_free_tier === true)
  }, [isGeminiBackend, backend, geminiModelsData, geminiPolicies, ollamaModelsData?.models])

  useEffect(() => {
    if (desiredModelRef.current) {
      if (availableModels.includes(desiredModelRef.current)) {
        setModel(desiredModelRef.current)
        desiredModelRef.current = null
      } else if (availableModels.length > 0) {
        desiredModelRef.current = null
        if (!availableModels.includes(model)) setModel(availableModels[0])
      }
    } else if (streamStatus === 'idle' && availableModels.length > 0 && !availableModels.includes(model)) {
      setModel(availableModels[0])
    }
  }, [availableModels, model, streamStatus]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (pendingSubmitRef.current && model && prompt && apiKey && streamStatus === 'idle') {
      pendingSubmitRef.current = false
      doSubmit()
    }
  }, [model]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── localStorage key for this tab ─────────────────────────────────────────────
  const lsKey = `veronex:test:tab:${tabKey}`

  // ── Stream reader shared between doSubmit and reconnectStream ─────────────────
  async function consumeStream(reader: ReadableStreamDefaultReader<Uint8Array>) {
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
        if (data === '[DONE]') { setStreamStatus('done'); reader.cancel(); return }
        try {
          const chunk: OpenAIChunk = JSON.parse(data)
          if (chunk.error?.message) throw new Error(chunk.error.message)
          if (chunk.id && !jobIdRef.current) {
            jobIdRef.current = chunk.id.replace('chatcmpl-', '')
            try { localStorage.setItem(lsKey, JSON.stringify({ jobId: jobIdRef.current })) } catch { /* non-fatal */ }
          }
          const content = chunk.choices?.[0]?.delta?.content
          if (content) setTokens((prev) => [...prev, content])
        } catch (err) {
          if (err instanceof SyntaxError) continue
          throw err
        }
      }
    }
    setStreamStatus('done')
  }

  // ── Reconnect an existing job via GET /v1/jobs/{id}/stream ────────────────────
  async function reconnectStream(savedJobId: string) {
    if (!apiKey) return
    stoppedRef.current = false
    readerRef.current?.cancel()
    readerRef.current = null
    jobIdRef.current = savedJobId
    setTokens([])
    setErrorMsg('')
    setStreamStatus('streaming')
    try {
      const resp = await fetch(`${BASE}/v1/jobs/${savedJobId}/stream`, {
        headers: { 'X-API-Key': apiKey },
      })
      if (!resp.ok || !resp.body) throw new Error(`${resp.status} ${resp.statusText}`)
      const reader = resp.body.getReader()
      readerRef.current = reader
      await consumeStream(reader)
    } catch (err) {
      if (stoppedRef.current) return
      setErrorMsg(err instanceof Error ? err.message : 'Unknown error')
      setStreamStatus('error')
    }
  }

  // ── Restore from localStorage on mount ────────────────────────────────────────
  useEffect(() => {
    try {
      const saved = localStorage.getItem(lsKey)
      if (saved) {
        const { jobId } = JSON.parse(saved)
        if (jobId) reconnectStream(jobId)
      }
    } catch { /* ignore */ }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Core submit ───────────────────────────────────────────────────────────────
  async function doSubmit() {
    if (!prompt.trim() || !model || !apiKey || streamStatus === 'streaming') return

    stoppedRef.current = false
    readerRef.current?.cancel()
    readerRef.current = null
    jobIdRef.current = null
    setTokens([])
    setErrorMsg('')
    setStreamStatus('streaming')

    try {
      const resp = await fetch(`${BASE}/v1/chat/completions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-API-Key': apiKey },
        body: JSON.stringify({
          model,
          messages: [{ role: 'user', content: prompt.trim() }],
          backend,
          source: 'test',
          stream: true,
        }),
      })

      if (!resp.ok || !resp.body) throw new Error(`${resp.status} ${resp.statusText}`)

      const reader = resp.body.getReader()
      readerRef.current = reader
      await consumeStream(reader)
    } catch (err) {
      if (stoppedRef.current) return
      setErrorMsg(err instanceof Error ? err.message : t('common.unknownError'))
      setStreamStatus('error')
    }
  }

  async function handleStop() {
    stoppedRef.current = true
    readerRef.current?.cancel()
    readerRef.current = null
    if (jobIdRef.current) {
      try { await api.cancelJob(jobIdRef.current) } catch { /* non-fatal */ }
      jobIdRef.current = null
    }
    setStreamStatus('done')
  }

  function handleReset() {
    stoppedRef.current = true
    readerRef.current?.cancel()
    readerRef.current = null
    jobIdRef.current = null
    setTokens([])
    setErrorMsg('')
    setStreamStatus('idle')
    try { localStorage.removeItem(lsKey) } catch { /* non-fatal */ }
  }

  const output = tokens.join('')
  const isRunning = streamStatus === 'streaming'

  return (
    <div className={hidden ? 'hidden' : undefined}>
      <form onSubmit={(e) => { e.preventDefault(); doSubmit() }} className="space-y-4">
        {/* Backend + Model */}
        <div className="grid grid-cols-2 gap-4">
          <div className="space-y-1.5">
            <Label>{t('test.backend')}</Label>
            <Select value={backend} onValueChange={(v) => { setBackend(v); setModel('') }} disabled={isRunning}>
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
              onValueChange={setModel}
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

        {/* Actions */}
        <div className="flex gap-3 flex-wrap">
          <Button type="submit" disabled={!prompt.trim() || !model || !apiKey || isRunning}>
            {isRunning
              ? <><Loader2 className="h-4 w-4 animate-spin mr-2" />{t('test.streaming')}</>
              : <><Send className="h-4 w-4 mr-2" />{t('test.run')}</>}
          </Button>
          {isRunning && (
            <Button type="button" variant="outline" onClick={handleStop}>
              <Square className="h-4 w-4 mr-2" fill="currentColor" />{t('test.stop')}
            </Button>
          )}
          {!isRunning && streamStatus !== 'idle' && (
            <Button type="button" variant="outline" onClick={handleReset}>
              <X className="h-4 w-4 mr-2" />{t('test.reset')}
            </Button>
          )}
          {(streamStatus === 'done' || streamStatus === 'error') && (
            <Button type="button" variant="outline" onClick={doSubmit}
              disabled={!prompt.trim() || !model || !apiKey}>
              <RotateCcw className="h-4 w-4 mr-2" />{t('test.runAgain')}
            </Button>
          )}
        </div>
      </form>

      {/* Output */}
      {(output || isRunning || streamStatus === 'done') && (
        <div className="pt-3 mt-4 border-t border-border space-y-2">
          <div className="flex items-center justify-between">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
              {t('test.output')}
            </p>
            {isRunning && (
              <span className="flex items-center gap-1.5 text-xs text-status-info-fg">
                <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse" />
                {t('test.streaming')}
              </span>
            )}
            {streamStatus === 'done' && (
              <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30">
                {t('test.complete')}
              </Badge>
            )}
          </div>
          <div className="text-sm text-text-bright font-mono leading-relaxed min-h-[2rem]">
            {renderWithMermaid(output, isRunning)}
          </div>
        </div>
      )}

      {/* Error */}
      {streamStatus === 'error' && (
        <div className="pt-3 mt-4 border-t border-border">
          <p className="font-semibold text-sm text-destructive">{t('test.errorTitle')}</p>
          <p className="text-sm mt-1 text-destructive/80">{errorMsg}</p>
        </div>
      )}
    </div>
  )
}

// ── ApiTestPanel — tabbed container ────────────────────────────────────────────

interface Props {
  retryParams?: RetryParams | null
  onRetryConsumed?: () => void
}

export function ApiTestPanel({ retryParams, onRetryConsumed }: Props) {
  const { t } = useTranslation()
  const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

  const [apiKey, setApiKey] = useState(process.env.NEXT_PUBLIC_VERONEX_ADMIN_KEY ?? '')
  const [showApiKey, setShowApiKey] = useState(false)

  // Tab management
  const [tabs, setTabs] = useState<TabMeta[]>([{ id: 1, streamStatus: 'idle' }])
  const [activeId, setActiveId] = useState(1)
  const nextIdRef = useRef(2)

  function addTab() {
    if (tabs.length >= MAX_TABS) return
    const id = nextIdRef.current++
    setTabs((prev) => [...prev, { id, streamStatus: 'idle' }])
    setActiveId(id)
  }

  function closeTab(id: number) {
    setTabs((prev) => {
      const remaining = prev.filter((t) => t.id !== id)
      if (remaining.length === 0) {
        const newId = nextIdRef.current++
        setActiveId(newId)
        return [{ id: newId, streamStatus: 'idle' }]
      }
      if (activeId === id) {
        const idx = prev.findIndex((t) => t.id === id)
        const next = prev[idx + 1] ?? prev[idx - 1]
        setActiveId(next.id)
      }
      return remaining
    })
  }

  function handleStatusChange(id: number, status: StreamStatus) {
    setTabs((prev) => prev.map((t) => t.id === id ? { ...t, streamStatus: status } : t))
  }

  return (
    <Card>
      <CardContent className="p-5 space-y-4">
        {/* API Key — shared across all tabs */}
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
        </div>

        {/* Tab bar */}
        <div className="flex items-center gap-1 border-b border-border pb-0 -mb-1">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-t-md border border-b-0 cursor-pointer select-none transition-colors ${
                tab.id === activeId
                  ? 'bg-card border-border text-foreground'
                  : 'bg-muted/40 border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/70'
              }`}
              onClick={() => setActiveId(tab.id)}
            >
              {tab.streamStatus === 'streaming' && (
                <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse shrink-0" />
              )}
              <span>#{tab.id}</span>
              {tabs.length > 1 && (
                <button
                  type="button"
                  className="ml-0.5 rounded hover:bg-destructive/20 hover:text-destructive p-0.5 -mr-1"
                  onClick={(e) => { e.stopPropagation(); closeTab(tab.id) }}
                  title="Close tab"
                >
                  <X className="h-3 w-3" />
                </button>
              )}
            </div>
          ))}
          {tabs.length < MAX_TABS && (
            <button
              type="button"
              className="flex items-center justify-center h-7 w-7 rounded text-muted-foreground hover:text-foreground hover:bg-muted/70 transition-colors"
              onClick={addTab}
              title="New tab"
            >
              <Plus className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        {/* Sessions — all mounted, only active one is visible */}
        {tabs.map((tab) => (
          <TestSession
            key={tab.id}
            hidden={tab.id !== activeId}
            apiKey={apiKey}
            BASE={BASE}
            tabKey={tab.id}
            retryParams={tab.id === activeId ? retryParams : null}
            onRetryConsumed={tab.id === activeId ? onRetryConsumed : undefined}
            onStatusChange={(status) => handleStatusChange(tab.id, status)}
          />
        ))}
      </CardContent>
    </Card>
  )
}
