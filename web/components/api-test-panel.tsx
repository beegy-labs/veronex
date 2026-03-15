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
import { PROVIDER_OLLAMA, PROVIDER_GEMINI } from '@/lib/constants'
import type { OpenAIChunk, Run, ProviderOption } from '@/components/api-test-types'
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

  const authUser = getAuthUser()

  // ── Shared form state ─────────────────────────────────────────────────────────
  const [providerType, setProviderType] = useState<string>(PROVIDER_OLLAMA)
  const [model, setModel] = useState('')
  const [prompt, setPrompt] = useState('')
  const [images, setImages] = useState<string[]>([])       // raw base64 (no data URL prefix)
  const [isCompressing, setIsCompressing] = useState(false)

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
  const MAX_IMAGES = 4
  const MAX_FILE_BYTES = 10 * 1024 * 1024

  const handleImageAdd = useCallback(async (files: FileList) => {
    const remaining = MAX_IMAGES - images.length
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
      setImages((prev) => [...prev, ...compressed].slice(0, MAX_IMAGES))
    } catch {
      // Compression failure — skip silently, file not added
    } finally {
      setIsCompressing(false)
    }
  }, [images.length])

  const handleImageRemove = useCallback((index: number) => {
    setImages((prev) => prev.filter((_, i) => i !== index))
  }, [])

  // ── Run handler ───────────────────────────────────────────────────────────────
  async function handleRun() {
    if (!prompt.trim() || !model) return
    if (!isLoggedIn()) return

    if (runs.length >= MAX_RUNS) {
      const oldest = runs[0]
      const oldReader = readersRef.current.get(oldest.id)
      if (oldReader) { oldReader.cancel(); readersRef.current.delete(oldest.id) }
      dispatch({ type: 'REMOVE', id: oldest.id })
    }

    const runId = nextIdRef.current++
    const currentImages = images.length > 0 ? [...images] : undefined

    const newRun: Run = {
      id: runId,
      prompt: prompt.trim(),
      model,
      provider_type: providerType,
      status: 'streaming',
      text: '',
      errorMsg: '',
      images: currentImages,
    }
    dispatch({ type: 'ADD', run: newRun })
    setActiveRunId(runId)

    const jobIdRef = { current: null as string | null }

    try {
      const resp = await fetch(`${BASE}/v1/test/completions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({
          model,
          messages: [{ role: 'user', content: prompt.trim() }],
          provider_type: providerType,
          stream: true,
          ...(currentImages && currentImages.length > 0 && { images: currentImages }),
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

  function handleRerun(run: Run) {
    setPrompt(run.prompt)
    setProviderType(run.provider_type)
    setModel(run.model)
    handleRun()
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
          isCompressing={isCompressing}
          availableOptions={availableOptions}
          availableModels={availableModels}
          isGeminiProvider={isGeminiProvider}
          isAnyStreaming={isAnyStreaming}
          canRun={canRun}
          authUsername={authUser?.username ?? null}
          onProviderChange={setProviderType}
          onModelChange={setModel}
          onPromptChange={setPrompt}
          onImagesChange={setImages}
          onImageAdd={handleImageAdd}
          onImageRemove={handleImageRemove}
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
