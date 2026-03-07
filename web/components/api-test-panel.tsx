'use client'

import { useState, useRef, useEffect, useReducer, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { isLoggedIn, getAuthUser } from '@/lib/auth'
import { providersQuery, ollamaModelsQuery, geminiModelsQuery, geminiPoliciesQuery } from '@/lib/queries/providers'
import type { RetryParams } from '@/lib/types'
import { Send, Loader2, X, Square, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'
import { renderWithMermaid } from '@/components/mermaid-block'
import { BASE } from '@/lib/api'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI } from '@/lib/constants'

// ── Types ──────────────────────────────────────────────────────────────────────

interface OpenAIChunk {
  id?: string
  choices?: { delta?: { content?: string }; finish_reason?: string | null }[]
  error?: { message?: string }
}

type ProviderOption = { value: string; label: string; isGemini: boolean }
type StreamStatus = 'idle' | 'streaming' | 'done' | 'error'

interface Run {
  id: number
  prompt: string
  model: string
  provider_type: string
  status: StreamStatus
  text: string
  errorMsg: string
}

type RunAction =
  | { type: 'APPEND'; id: number; token: string }
  | { type: 'SET_STATUS'; id: number; status: StreamStatus; errorMsg?: string }
  | { type: 'ADD'; run: Run }
  | { type: 'REMOVE'; id: number }

function runsReducer(state: Run[], action: RunAction): Run[] {
  switch (action.type) {
    case 'ADD':
      return [...state, action.run]
    case 'REMOVE':
      return state.filter((r) => r.id !== action.id)
    case 'APPEND':
      return state.map((r) =>
        r.id === action.id ? { ...r, text: r.text + action.token } : r
      )
    case 'SET_STATUS':
      return state.map((r) =>
        r.id === action.id
          ? { ...r, status: action.status, errorMsg: action.errorMsg ?? r.errorMsg }
          : r
      )
    default:
      return state
  }
}

const MAX_RUNS = 10

// ── ApiTestPanel ───────────────────────────────────────────────────────────────

interface Props {
  retryParams?: RetryParams | null
  onRetryConsumed?: () => void
}

export function ApiTestPanel({ retryParams, onRetryConsumed }: Props) {
  const { t } = useTranslation()

  const authUser = getAuthUser()

  // ── Shared form state ─────────────────────────────────────────────────────────
  const [providerType, setProviderType] = useState<string>(PROVIDER_OLLAMA)
  const [model, setModel] = useState('')
  const [prompt, setPrompt] = useState('')

  // ── Run state ─────────────────────────────────────────────────────────────────
  const [runs, dispatch] = useReducer(runsReducer, [])
  const [activeRunId, setActiveRunId] = useState<number | null>(null)
  const nextIdRef = useRef(1)

  // Map from run id → active reader (for cancellation)
  const readersRef = useRef<Map<number, ReadableStreamDefaultReader<Uint8Array>>>(new Map())

  // ── Providers ─────────────────────────────────────────────────────────────────
  const { data: providers } = useQuery(providersQuery)

  const availableOptions = useMemo((): ProviderOption[] => {
    if (!providers) return [{ value: 'ollama', label: 'Ollama', isGemini: false }]
    const opts: ProviderOption[] = []
    if (providers.some((b) => b.is_active && b.provider_type === PROVIDER_OLLAMA)) {
      opts.push({ value: 'ollama', label: 'Ollama', isGemini: false })
    }
    if (providers.some((b) => b.is_active && b.provider_type === PROVIDER_GEMINI && b.is_free_tier)) {
      opts.push({ value: 'gemini-free', label: t('test.geminiFree'), isGemini: true })
    }
    if (providers.some((b) => b.is_active && b.provider_type === PROVIDER_GEMINI && !b.is_free_tier)) {
      opts.push({ value: 'gemini', label: t('test.gemini'), isGemini: true })
    }
    return opts.length > 0 ? opts : [{ value: 'ollama', label: 'Ollama', isGemini: false }]
  }, [providers, t])

  const isGeminiProvider = availableOptions.find((o) => o.value === providerType)?.isGemini ?? false

  useEffect(() => {
    if (!providers) return
    if (!availableOptions.find((o) => o.value === providerType)) {
      setProviderType(availableOptions[0].value)
      setModel('')
    }
  }, [availableOptions, providerType, providers])

  // ── Models ────────────────────────────────────────────────────────────────────
  const { data: ollamaModelsData } = useQuery({
    ...ollamaModelsQuery,
    enabled: !isGeminiProvider,
  })

  const { data: geminiModelsData } = useQuery({
    ...geminiModelsQuery,
    enabled: isGeminiProvider,
  })

  const { data: geminiPolicies } = useQuery({
    ...geminiPoliciesQuery,
    enabled: isGeminiProvider,
  })

  const availableModels = useMemo(() => {
    if (!isGeminiProvider) return ollamaModelsData?.models.map((m) => m.model_name) ?? []
    const allModels = geminiModelsData?.models.map((m) => m.model_name) ?? []
    if (providerType !== "gemini-free") return allModels
    const policyMap = new Map(
      (geminiPolicies ?? []).filter((p) => p.model_name !== '*').map((p) => [p.model_name, p])
    )
    return allModels.filter((name) => policyMap.get(name)?.available_on_free_tier === true)
  }, [isGeminiProvider, providerType, geminiModelsData, geminiPolicies, ollamaModelsData?.models])

  useEffect(() => {
    if (availableModels.length > 0 && !availableModels.includes(model)) {
      setModel(availableModels[0])
    }
  }, [availableModels]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Retry params ─────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!retryParams) return
    setPrompt(retryParams.prompt)
    setProviderType(retryParams.provider_type)
    if (availableModels.includes(retryParams.model)) {
      setModel(retryParams.model)
    }
    onRetryConsumed?.()
  }, [retryParams]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Cleanup on unmount ────────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      for (const reader of readersRef.current.values()) {
        reader.cancel()
      }
    }
  }, [])

  // ── SSE consumer ─────────────────────────────────────────────────────────────
  async function consumeStream(
    runId: number,
    reader: ReadableStreamDefaultReader<Uint8Array>,
    jobIdRef: { current: string | null },
  ) {
    const decoder = new TextDecoder()
    let buf = ''
    try {
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
            dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
            reader.cancel()
            readersRef.current.delete(runId)
            return
          }
          try {
            const chunk: OpenAIChunk = JSON.parse(data)
            if (chunk.error?.message) throw new Error(chunk.error.message)
            if (chunk.id && !jobIdRef.current) {
              jobIdRef.current = chunk.id.replace('chatcmpl-', '')
            }
            const content = chunk.choices?.[0]?.delta?.content
            if (content) dispatch({ type: 'APPEND', id: runId, token: content })
          } catch (err) {
            if (err instanceof SyntaxError) continue
            throw err
          }
        }
      }
      dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
    } catch (err) {
      dispatch({
        type: 'SET_STATUS',
        id: runId,
        status: 'error',
        errorMsg: err instanceof Error ? err.message : t('common.unknownError'),
      })
    } finally {
      readersRef.current.delete(runId)
    }
  }

  // ── Run handler ───────────────────────────────────────────────────────────────
  async function handleRun() {
    if (!prompt.trim() || !model) return
    if (!isLoggedIn()) return

    if (runs.length >= MAX_RUNS) {
      // Remove oldest run
      const oldest = runs[0]
      const oldReader = readersRef.current.get(oldest.id)
      if (oldReader) { oldReader.cancel(); readersRef.current.delete(oldest.id) }
      dispatch({ type: 'REMOVE', id: oldest.id })
    }

    const runId = nextIdRef.current++
    const newRun: Run = {
      id: runId,
      prompt: prompt.trim(),
      model,
      provider_type: providerType,
      status: 'streaming',
      text: '',
      errorMsg: '',
    }
    dispatch({ type: 'ADD', run: newRun })
    setActiveRunId(runId)

    const jobIdRef = { current: null as string | null }

    try {
      const resp = await fetch(`${BASE}/v1/test/completions`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        credentials: 'include',
        body: JSON.stringify({
          model,
          messages: [{ role: 'user', content: prompt.trim() }],
          provider_type: providerType,
          stream: true,
        }),
      })

      if (!resp.ok || !resp.body) {
        throw new Error(`${resp.status} ${resp.statusText}`)
      }

      const reader = resp.body.getReader()
      readersRef.current.set(runId, reader)
      await consumeStream(runId, reader, jobIdRef)
    } catch (err) {
      dispatch({
        type: 'SET_STATUS',
        id: runId,
        status: 'error',
        errorMsg: err instanceof Error ? err.message : t('common.unknownError'),
      })
    }
  }

  function handleStop(runId: number) {
    const reader = readersRef.current.get(runId)
    if (reader) { reader.cancel(); readersRef.current.delete(runId) }
    dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
  }

  function handleCloseRun(runId: number) {
    const reader = readersRef.current.get(runId)
    if (reader) { reader.cancel(); readersRef.current.delete(runId) }
    dispatch({ type: 'REMOVE', id: runId })
    if (activeRunId === runId) {
      const remaining = runs.filter((r) => r.id !== runId)
      setActiveRunId(remaining.length > 0 ? remaining[remaining.length - 1].id : null)
    }
  }

  const activeRun = runs.find((r) => r.id === activeRunId) ?? null
  const canRun = isLoggedIn() && !!prompt.trim() && !!model
  const isAnyStreaming = runs.some((r) => r.status === 'streaming')

  return (
    <Card>
      <CardContent className="p-5 space-y-0">

        {/* ── Top: form ────────────────────────────────────────────────────────── */}
        <form onSubmit={(e) => { e.preventDefault(); handleRun() }} className="space-y-4 pb-4">
          {/* Provider + Model */}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <Label>{t('test.provider')}</Label>
              <Select
                value={providerType}
                onValueChange={(v) => { setProviderType(v); setModel('') }}
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
                onValueChange={setModel}
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
                onChange={(e) => setPrompt(e.target.value)}
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
          {authUser && (
            <p className="text-xs text-muted-foreground">
              {t('test.runningAs')}: <span className="font-medium text-foreground">{authUser.username}</span>
            </p>
          )}
        </form>

        {/* ── Divider ──────────────────────────────────────────────────────────── */}
        {runs.length > 0 && (
          <div className="border-t border-border pt-4 space-y-3">
            {/* Tab strip */}
            <div className="flex items-center gap-1 border-b border-border pb-0 -mb-1 flex-wrap">
              {runs.map((run) => (
                <div
                  key={run.id}
                  className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-t-md border border-b-0 cursor-pointer select-none transition-colors ${
                    run.id === activeRunId
                      ? 'bg-card border-border text-foreground'
                      : 'bg-muted/40 border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/70'
                  }`}
                  onClick={() => setActiveRunId(run.id)}
                >
                  {run.status === 'streaming' && (
                    <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse shrink-0" />
                  )}
                  {run.status === 'done' && (
                    <span className="h-1.5 w-1.5 rounded-full bg-status-success shrink-0" />
                  )}
                  {run.status === 'error' && (
                    <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
                  )}
                  <span>#{run.id}</span>
                  <button
                    type="button"
                    className="ml-0.5 rounded hover:bg-destructive/20 hover:text-destructive p-0.5 -mr-1"
                    onClick={(e) => { e.stopPropagation(); handleCloseRun(run.id) }}
                    title="Close"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              ))}
            </div>

            {/* Active run output */}
            {activeRun && (
              <div className="pt-1 space-y-2">
                {/* Run controls */}
                {activeRun.status === 'streaming' && (
                  <div className="flex items-center justify-between">
                    <span className="flex items-center gap-1.5 text-xs text-status-info-fg">
                      <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse" />
                      {t('test.streaming')}
                    </span>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => handleStop(activeRun.id)}
                    >
                      <Square className="h-3.5 w-3.5 mr-1.5" fill="currentColor" />
                      {t('test.stop')}
                    </Button>
                  </div>
                )}

                {activeRun.status === 'done' && (
                  <div className="flex items-center justify-between">
                    <Badge variant="outline" className="bg-status-success/15 text-status-success-fg border-status-success/30">
                      {t('test.complete')}
                    </Badge>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setPrompt(activeRun.prompt)
                        setProviderType(activeRun.provider_type)
                        setModel(activeRun.model)
                        handleRun()
                      }}
                      disabled={isAnyStreaming}
                    >
                      <RotateCcw className="h-3.5 w-3.5 mr-1.5" />
                      {t('test.runAgain')}
                    </Button>
                  </div>
                )}

                {/* Output */}
                {(activeRun.text.length > 0 || activeRun.status === 'streaming') && (
                  <div className="rounded-md border border-border bg-muted/20 p-3 min-h-[64px]">
                    <div className="text-sm text-foreground font-mono leading-relaxed">
                      {renderWithMermaid(activeRun.text, activeRun.status === 'streaming')}
                    </div>
                  </div>
                )}

                {/* Error */}
                {activeRun.status === 'error' && (
                  <div className="rounded-md border border-status-error/30 bg-status-error/5 p-3">
                    <p className="font-semibold text-sm text-status-error-fg">{t('test.errorTitle')}</p>
                    <p className="text-sm mt-1 text-status-error-fg/80">{activeRun.errorMsg}</p>
                  </div>
                )}

                {/* Prompt snapshot for context */}
                <p className="text-xs text-muted-foreground truncate">
                  <span className="font-medium">{activeRun.model}</span>
                  {' · '}
                  <span className="opacity-70">{activeRun.prompt.slice(0, 80)}{activeRun.prompt.length > 80 ? '…' : ''}</span>
                </p>
              </div>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
