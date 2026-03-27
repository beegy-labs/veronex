'use client'

import { useState, useRef, useEffect, useReducer, useMemo, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { isLoggedIn, getAuthUser } from '@/lib/auth'
import { providersQuery, ollamaModelsQuery, geminiModelsQuery, geminiPoliciesQuery } from '@/lib/queries/providers'
import type { RetryParams } from '@/lib/types'
import { Card, CardContent } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { BASE } from '@/lib/api'
import { compressImage } from '@/lib/compress-image'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI, DEFAULT_MAX_IMAGES, MAX_FILE_BYTES } from '@/lib/constants'
import { useLabSettings } from '@/components/lab-settings-provider'
import type { OpenAIChunk, Run, ProviderOption, Endpoint } from '@/components/api-test-types'
import { runsReducer, MAX_RUNS } from '@/components/api-test-types'
import { ApiTestForm } from '@/components/api-test-form'
import { ApiTestRuns } from '@/components/api-test-runs'

// ── ApiTestPanel ───────────────────────────────────────────────────────────────

interface Props {
  retryParams?: RetryParams | null
  onRetryConsumed?: () => void
}

export function ApiTestPanel({ retryParams, onRetryConsumed }: Props) {
  const { t } = useTranslation()
  const { labSettings } = useLabSettings()

  const authUser = getAuthUser()

  // ── Shared form state ─────────────────────────────────────────────────────────
  const [providerType, setProviderType] = useState<string>(PROVIDER_OLLAMA)
  const [model, setModel] = useState('')
  const [prompt, setPrompt] = useState('')
  const [images, setImages] = useState<string[]>([])       // raw base64 (no data URL prefix)
  const [isCompressing, setIsCompressing] = useState(false)
  const [endpoint, setEndpoint] = useState<Endpoint>('/v1/chat/completions')
  const [useApiKey, setUseApiKey] = useState(false)
  const [apiKeyValue, setApiKeyValue] = useState('')

  // ── Run state ─────────────────────────────────────────────────────────────────
  const [runs, dispatch] = useReducer(runsReducer, [])
  const [activeRunId, setActiveRunId] = useState<number | null>(null)
  const nextIdRef = useRef(1)

  // Map from run id → active reader (for cancellation)
  const readersRef = useRef<Map<number, ReadableStreamDefaultReader<Uint8Array>>>(new Map())

  // ── Providers ─────────────────────────────────────────────────────────────────
  const { data: providersData } = useQuery(providersQuery())
  const providers = providersData?.providers

  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const availableOptions = useMemo((): ProviderOption[] => {
    if (!providers) return [{ value: 'ollama', label: 'Ollama', isGemini: false }]
    const opts: ProviderOption[] = []
    if (providers.some((b) => b.is_active && b.provider_type === PROVIDER_OLLAMA)) {
      opts.push({ value: 'ollama', label: 'Ollama', isGemini: false })
    }
    if (geminiEnabled && providers.some((b) => b.is_active && b.provider_type === PROVIDER_GEMINI && b.is_free_tier)) {
      opts.push({ value: 'gemini-free', label: t('test.geminiFree'), isGemini: true })
    }
    if (geminiEnabled && providers.some((b) => b.is_active && b.provider_type === PROVIDER_GEMINI && !b.is_free_tier)) {
      opts.push({ value: 'gemini', label: t('test.gemini'), isGemini: true })
    }
    return opts.length > 0 ? opts : [{ value: 'ollama', label: 'Ollama', isGemini: false }]
  }, [providers, t, geminiEnabled])

  const isGeminiProvider = availableOptions.find((o) => o.value === providerType)?.isGemini ?? false

  useEffect(() => {
    if (!providers) return
    if (!availableOptions.find((o) => o.value === providerType)) {
      setProviderType(availableOptions[0].value)
      setModel('')
    }
  }, [availableOptions, providerType, providers])

  // Auto-switch endpoint when provider type changes
  useEffect(() => {
    if (isGeminiProvider && endpoint !== '/v1/chat/completions' && endpoint !== '/v1beta/models') {
      setEndpoint('/v1/chat/completions')
    }
    if (!isGeminiProvider && endpoint === '/v1beta/models') {
      setEndpoint('/v1/chat/completions')
    }
  }, [isGeminiProvider, endpoint])

  // ── Models ────────────────────────────────────────────────────────────────────
  const { data: ollamaModelsData } = useQuery({
    ...ollamaModelsQuery(),
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
  }, [availableModels, model])

  // ── Retry params ─────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!retryParams) return
    setPrompt(retryParams.prompt)
    setProviderType(retryParams.provider_type)
    if (availableModels.includes(retryParams.model)) {
      setModel(retryParams.model)
    }
    onRetryConsumed?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [retryParams])

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

  // ── Image handlers ────────────────────────────────────────────────────────────
  const maxImages = labSettings?.max_images_per_request ?? DEFAULT_MAX_IMAGES

  const handleImageAdd = useCallback(async (files: FileList) => {
    const remaining = maxImages - images.length
    if (remaining <= 0) return

    const toProcess = Array.from(files).slice(0, remaining)
    const oversized = toProcess.filter((f) => f.size > MAX_FILE_BYTES)
    if (oversized.length > 0) {
      // Silently skip oversized files — UX toast would require a toast system
      return
    }

    setIsCompressing(true)
    try {
      const compressed = await Promise.all(toProcess.map((f) => compressImage(f)))
      setImages((prev) => [...prev, ...compressed].slice(0, maxImages))
    } catch {
      // Compression failure — skip silently, file not added
    } finally {
      setIsCompressing(false)
    }
  }, [images.length, maxImages])

  const handleImageRemove = useCallback((index: number) => {
    setImages((prev) => prev.filter((_, i) => i !== index))
  }, [])

  // ── Run handler ───────────────────────────────────────────────────────────────
  interface RunParams {
    prompt: string; model: string; providerType: string
    endpoint: Endpoint; useApiKey: boolean; images?: string[]
  }

  async function executeRun(p: RunParams) {
    if (!p.prompt.trim() || !p.model) return
    if (!isLoggedIn()) return

    if (runs.length >= MAX_RUNS) {
      const oldest = runs[0]
      const oldReader = readersRef.current.get(oldest.id)
      if (oldReader) { oldReader.cancel(); readersRef.current.delete(oldest.id) }
      dispatch({ type: 'REMOVE', id: oldest.id })
    }

    const runId = nextIdRef.current++

    const newRun: Run = {
      id: runId,
      prompt: p.prompt.trim(),
      model: p.model,
      provider_type: p.providerType,
      endpoint: p.endpoint,
      useApiKey: p.useApiKey,
      status: 'streaming',
      text: '',
      errorMsg: '',
      images: p.images,
    }
    dispatch({ type: 'ADD', run: newRun })
    setActiveRunId(runId)

    const jobIdRef = { current: null as string | null }

    try {
      const isStreaming = p.endpoint === '/v1/chat/completions' || p.endpoint === '/api/chat' || p.endpoint === '/api/generate'
      let url: string
      const headers: Record<string, string> = { 'Content-Type': 'application/json' }

      url = `${BASE}${p.endpoint}`
      if (p.useApiKey && apiKeyValue.trim()) {
        if (p.endpoint === '/v1/chat/completions' || p.endpoint === '/v1beta/models') {
          headers['Authorization'] = `Bearer ${apiKeyValue.trim()}`
        } else {
          headers['X-API-Key'] = apiKeyValue.trim()
        }
      }
      // Session auth (JWT cookie) is sent automatically by the browser.

      let body: Record<string, unknown>
      if (p.endpoint === '/v1beta/models') {
        // Gemini native: POST /v1beta/models/{model}:generateContent
        url = `${BASE}/v1beta/models/${encodeURIComponent(p.model)}:generateContent`
        body = { contents: [{ parts: [{ text: p.prompt.trim() }] }] }
      } else if (p.endpoint === '/api/generate') {
        body = { model: p.model, prompt: p.prompt.trim(), stream: isStreaming }
      } else if (p.endpoint === '/api/chat') {
        body = {
          model: p.model,
          messages: [{ role: 'user', content: p.prompt.trim() }],
          stream: isStreaming,
        }
      } else {
        body = {
          model: p.model,
          messages: [{ role: 'user', content: p.prompt.trim() }],
          provider_type: p.providerType,
          stream: isStreaming,
          ...(p.images && p.images.length > 0 && { images: p.images }),
        }
      }

      const resp = await fetch(url, {
        method: 'POST',
        headers,
        ...(!p.useApiKey && { credentials: 'include' as RequestCredentials }),
        body: JSON.stringify(body),
      })

      if (!resp.ok) {
        throw new Error(`${resp.status} ${resp.statusText}`)
      }

      if (isStreaming && resp.body) {
        const reader = resp.body.getReader()
        readersRef.current.set(runId, reader)
        await consumeStream(runId, reader, jobIdRef)
      } else {
        const json = await resp.json()
        const text = p.endpoint === '/v1beta/models'
          ? (json.candidates?.[0]?.content?.parts?.[0]?.text ?? JSON.stringify(json))
          : p.endpoint === '/api/generate'
            ? (json.response ?? '')
            : (json.message?.content ?? '')
        dispatch({ type: 'APPEND', id: runId, token: text })
        dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
      }
    } catch (err) {
      dispatch({
        type: 'SET_STATUS',
        id: runId,
        status: 'error',
        errorMsg: err instanceof Error ? err.message : t('common.unknownError'),
      })
    }
  }

  function handleRun() {
    executeRun({
      prompt, model, providerType, endpoint, useApiKey,
      images: images.length > 0 ? [...images] : undefined,
    })
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

  function handleRerun(run: Run) {
    setPrompt(run.prompt)
    setProviderType(run.provider_type)
    setModel(run.model)
    setEndpoint(run.endpoint)
    setUseApiKey(run.useApiKey)
    executeRun({
      prompt: run.prompt,
      model: run.model,
      providerType: run.provider_type,
      endpoint: run.endpoint,
      useApiKey: run.useApiKey,
      images: run.images,
    })
  }

  const canRun = isLoggedIn() && !!prompt.trim() && !!model
  const isAnyStreaming = runs.some((r) => r.status === 'streaming')

  return (
    <Card>
      <CardContent className="p-5 space-y-0">
        <ApiTestForm
          providerType={providerType}
          model={model}
          prompt={prompt}
          images={images}
          maxImages={maxImages}
          isCompressing={isCompressing}
          availableOptions={availableOptions}
          availableModels={availableModels}
          isGeminiProvider={isGeminiProvider}
          canRun={canRun}
          authUsername={authUser?.username ?? null}
          endpoint={endpoint}
          useApiKey={useApiKey}
          apiKeyValue={apiKeyValue}
          onProviderChange={setProviderType}
          onModelChange={setModel}
          onPromptChange={setPrompt}
          onImageAdd={handleImageAdd}
          onImageRemove={handleImageRemove}
          onEndpointChange={setEndpoint}
          onUseApiKeyChange={setUseApiKey}
          onApiKeyValueChange={setApiKeyValue}
          onRun={handleRun}
        />

        <ApiTestRuns
          runs={runs}
          activeRunId={activeRunId}
          isAnyStreaming={isAnyStreaming}
          onSelectRun={setActiveRunId}
          onCloseRun={handleCloseRun}
          onStop={handleStop}
          onRerun={handleRerun}
        />
      </CardContent>
    </Card>
  )
}
