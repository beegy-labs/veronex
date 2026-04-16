'use client'

import { useState, useRef, useEffect, useReducer, useMemo, useCallback, useEffectEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { isLoggedIn, getAuthUser } from '@/lib/auth'
import { providersQuery, ollamaModelsQuery, geminiModelsQuery, geminiPoliciesQuery, globalModelSettingsQuery } from '@/lib/queries/providers'
import type { RetryParams, ConversationDetail } from '@/lib/types'
import { Card, CardContent } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { BASE } from '@/lib/api'
import { compressImage } from '@/lib/compress-image'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI, DEFAULT_MAX_IMAGES, MAX_FILE_BYTES } from '@/lib/constants'
import { isModelEnabled } from '@/lib/models'
import { iterSseLines } from '@/lib/sse'
import { useLabSettings } from '@/components/lab-settings-provider'
import type { OpenAIChunk, Run, ProviderOption, Endpoint, ConversationMessage, ConversationSession, TestMode } from './api-test-types'
import { runsReducer, MAX_RUNS, MAX_CONV_SESSIONS } from './api-test-types'
import { ApiTestForm } from './api-test-form'
import { ApiTestRuns } from './api-test-runs'
import { ApiTestConversation } from './api-test-conversation'

const EMPTY_MESSAGES: ConversationMessage[] = []

// ── ApiTestPanel ───────────────────────────────────────────────────────────────

interface Props {
  retryParams?: RetryParams | null
  onRetryConsumed?: () => void
  onTurnComplete?: () => void
  continueConversation?: ConversationDetail | null
  onContinueConsumed?: () => void
}

export function ApiTestPanel({ retryParams, onRetryConsumed, onTurnComplete, continueConversation, onContinueConsumed }: Props) {
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
  const [useMcp, setUseMcp] = useState(true)

  // ── Mode ─────────────────────────────────────────────────────────────────────
  const [mode, setMode] = useState<TestMode>('single')

  // ── Run state (single mode) ───────────────────────────────────────────────────
  const [runs, dispatch] = useReducer(runsReducer, [])
  const [activeRunId, setActiveRunId] = useState<number | null>(null)
  const nextIdRef = useRef(1)

  // Map from run id → active reader (for cancellation)
  const readersRef = useRef<Map<number, ReadableStreamDefaultReader<Uint8Array>>>(new Map())

  // ── Conversation state ────────────────────────────────────────────────────────
  const [conversationSessions, setConversationSessions] = useState<ConversationSession[]>([])
  const [activeConvSessionId, setActiveConvSessionId] = useState<number | null>(null)
  const convNextIdRef = useRef(1)
  const convReadersRef = useRef<Map<number, ReadableStreamDefaultReader<Uint8Array>>>(new Map())

  // ── Providers ─────────────────────────────────────────────────────────────────
  const { data: providersData } = useQuery(providersQuery())
  const providers = providersData?.providers

  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const availableOptions = useMemo((): ProviderOption[] => {
    if (!providers) return [{ value: 'ollama', label: t('jobs.providerOllama'), isGemini: false }]
    const opts: ProviderOption[] = []
    if (providers.some((b) => b.provider_type === PROVIDER_OLLAMA)) {
      opts.push({ value: 'ollama', label: t('jobs.providerOllama'), isGemini: false })
    }
    if (geminiEnabled && providers.some((b) => b.provider_type === PROVIDER_GEMINI && b.is_free_tier)) {
      opts.push({ value: 'gemini-free', label: t('test.geminiFree'), isGemini: true })
    }
    if (geminiEnabled && providers.some((b) => b.provider_type === PROVIDER_GEMINI && !b.is_free_tier)) {
      opts.push({ value: 'gemini', label: t('test.gemini'), isGemini: true })
    }
    return opts.length > 0 ? opts : [{ value: 'ollama', label: t('jobs.providerOllama'), isGemini: false }]
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

  // Conversation mode: force messages-capable endpoint
  useEffect(() => {
    if (mode === 'conversation' && endpoint === '/api/generate') {
      setEndpoint('/v1/chat/completions')
    }
    if (mode === 'conversation' && endpoint === '/v1beta/models') {
      setEndpoint('/v1/chat/completions')
    }
  }, [mode, endpoint])

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

  const { data: globalModelSettings } = useQuery({
    ...globalModelSettingsQuery,
    enabled: !isGeminiProvider,
  })

  const modelContextWindows = useMemo<Record<string, number>>(() => {
    if (isGeminiProvider) return {}
    return Object.fromEntries(
      (ollamaModelsData?.models ?? [])
        .filter((m) => (m.max_ctx ?? 0) > 0)
        .map((m) => [m.model_name, m.max_ctx!])
    )
  }, [isGeminiProvider, ollamaModelsData?.models])

  const availableModels = useMemo(() => {
    if (!isGeminiProvider) {
      const disabledSet = new Set(
        (globalModelSettings ?? []).filter((s) => !s.is_enabled).map((s) => s.model_name)
      )
      return (ollamaModelsData?.models ?? [])
        .filter((m) => isModelEnabled(m) && !disabledSet.has(m.model_name))
        .map((m) => m.model_name)
    }
    const allModels = geminiModelsData?.models.map((m) => m.model_name) ?? []
    if (providerType !== "gemini-free") return allModels
    const policyMap = new Map(
      (geminiPolicies ?? []).filter((p) => p.model_name !== '*').map((p) => [p.model_name, p])
    )
    return allModels.filter((name) => policyMap.get(name)?.available_on_free_tier === true)
  }, [isGeminiProvider, providerType, geminiModelsData, geminiPolicies, ollamaModelsData?.models, globalModelSettings])

  useEffect(() => {
    if (availableModels.length > 0 && !availableModels.includes(model)) {
      setModel(availableModels[0])
    }
  }, [availableModels, model])

  // ── Retry params ─────────────────────────────────────────────────────────────
  const applyRetryParams = useEffectEvent(() => {
    if (!retryParams) return
    setPrompt(retryParams.prompt)
    setProviderType(retryParams.provider_type)
    if (availableModels.includes(retryParams.model)) {
      setModel(retryParams.model)
    }
    onRetryConsumed?.()
  })
  useEffect(() => { applyRetryParams() }, [retryParams])

  // ── Continue conversation ─────────────────────────────────────────────────────
  const applyContinueConversation = useEffectEvent(() => {
    if (!continueConversation) return
    setMode('conversation')
    if (conversationSessions.length >= MAX_CONV_SESSIONS) return
    const id = convNextIdRef.current++
    const messages: ConversationMessage[] = continueConversation.turns.flatMap((turn) => {
      const msgs: ConversationMessage[] = [{ role: 'user', content: turn.prompt }]
      if (turn.result) msgs.push({ role: 'assistant', content: turn.result, model: turn.model_name ?? undefined })
      return msgs
    })
    const newSess: ConversationSession = {
      id,
      messages,
      streamingText: '',
      status: 'idle',
      errorMsg: '',
      conversationId: continueConversation.id,
    }
    setConversationSessions((prev) => [...prev, newSess])
    setActiveConvSessionId(id)
    if (continueConversation.model_name && availableModels.includes(continueConversation.model_name)) {
      setModel(continueConversation.model_name)
    }
    onContinueConsumed?.()
  })
  useEffect(() => { applyContinueConversation() }, [continueConversation])

  // ── Cleanup on unmount ────────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      for (const reader of readersRef.current.values()) reader.cancel()
      for (const reader of convReadersRef.current.values()) reader.cancel()
    }
  }, [])

  // ── SSE consumer ─────────────────────────────────────────────────────────────
  const consumeStream = useCallback(async (
    runId: number,
    reader: ReadableStreamDefaultReader<Uint8Array>,
    jobIdRef: { current: string | null },
  ) => {
    try {
      for await (const { eventType, data } of iterSseLines(reader)) {
        if (data === '[DONE]') {
          dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
          reader.cancel()
          readersRef.current.delete(runId)
          return
        }
        if (eventType === 'error') throw new Error(data)
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
      dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
      onTurnComplete?.()
    } catch (err) {
      dispatch({
        type: 'SET_STATUS',
        id: runId,
        status: 'error',
        errorMsg: err instanceof Error ? err.message : t('common.unknownError'),
      })
      onTurnComplete?.()
    } finally {
      readersRef.current.delete(runId)
    }
  }, [onTurnComplete])

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

  // ── Conversation session management ──────────────────────────────────────────
  const handleNewConvSession = useCallback(() => {
    if (conversationSessions.length >= MAX_CONV_SESSIONS) return
    const id = convNextIdRef.current++
    const newSess: ConversationSession = { id, messages: [], streamingText: '', status: 'idle', errorMsg: '' }
    setConversationSessions((prev) => [...prev, newSess])
    setActiveConvSessionId(id)
  }, [conversationSessions.length])

  const handleCloseConvSession = useCallback((id: number) => {
    convReadersRef.current.get(id)?.cancel()
    convReadersRef.current.delete(id)
    setConversationSessions((prev) => {
      const remaining = prev.filter((s) => s.id !== id)
      setActiveConvSessionId((cur) => cur === id
        ? (remaining.length > 0 ? remaining[remaining.length - 1].id : null)
        : cur
      )
      return remaining
    })
  }, [])

  // ── Conversation handler ──────────────────────────────────────────────────────
  const activeConvSession = conversationSessions.find((s) => s.id === activeConvSessionId) ?? null

  const executeConversationTurn = useCallback(async () => {
    if ((!prompt.trim() && images.length === 0) || !model) return
    if (!isLoggedIn()) return

    // Auto-create first session if none
    let sid = activeConvSessionId
    let currentMessages: ConversationMessage[] = []
    if (sid === null) {
      if (conversationSessions.length >= MAX_CONV_SESSIONS) return
      const id = convNextIdRef.current++
      const newSess: ConversationSession = { id, messages: [], streamingText: '', status: 'idle', errorMsg: '' }
      setConversationSessions((prev) => [...prev, newSess])
      setActiveConvSessionId(id)
      sid = id
    } else {
      const sess = conversationSessions.find((s) => s.id === sid)
      if (!sess || sess.status === 'streaming') return
      currentMessages = sess.messages
    }

    const userContent = prompt.trim()
    const userImages = images.length > 0 ? [...images] : undefined
    const userMsg: ConversationMessage = { role: 'user', content: userContent, images: userImages }
    const updatedMessages = [...currentMessages, userMsg]

    setConversationSessions((prev) => prev.map((s) =>
      s.id === sid ? { ...s, messages: updatedMessages, streamingText: '', status: 'streaming', errorMsg: '' } : s
    ))
    setPrompt('')
    setImages([])

    const ep = (endpoint === '/api/generate' || endpoint === '/v1beta/models')
      ? '/v1/chat/completions'
      : endpoint

    // Images are not retained between turns in Ollama — each message is processed
    // independently. The assistant's analysis from turn 1 already captures image
    // context in text form. Re-sending historical images wastes bandwidth, inflates
    // the payload (causing stream timeouts), and confuses the model.
    // Only include images in the LAST (current) user message.
    const lastIdx = updatedMessages.length - 1
    const apiMessages = updatedMessages.map((m, idx) => ({
      role: m.role,
      content: m.content,
      ...(idx === lastIdx && m.images && m.images.length > 0 && { images: m.images }),
    }))

    const headers: Record<string, string> = { 'Content-Type': 'application/json' }
    if (useApiKey && apiKeyValue.trim()) {
      headers['Authorization'] = `Bearer ${apiKeyValue.trim()}`
    }

    // Include server conversation_id to continue the same conversation context
    const existingConvId = conversationSessions.find((s) => s.id === sid)?.conversationId
      ?? (sid === activeConvSessionId ? activeConvSession?.conversationId : undefined)

    const body: Record<string, unknown> = {
      model,
      messages: apiMessages,
      provider_type: providerType,
      stream: true,
      use_mcp: useMcp,
      ...(existingConvId && { conversation_id: existingConvId }),
    }

    let fullText = ''
    try {
      const resp = await fetch(`${BASE}${ep}`, {
        method: 'POST',
        headers,
        ...(!useApiKey && { credentials: 'include' as RequestCredentials }),
        body: JSON.stringify(body),
      })
      if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`)

      // Capture server conversation_id from response header and persist in session
      const serverConvId = resp.headers.get('x-conversation-id')
      if (serverConvId) {
        const prevConvId = conversationSessions.find((s) => s.id === sid)?.conversationId
          ?? (sid === activeConvSessionId ? activeConvSession?.conversationId : undefined)
        const renewed = prevConvId !== undefined && serverConvId !== prevConvId
        setConversationSessions((prev) => prev.map((s) => {
          if (s.id !== sid) return s
          const msgs = renewed
            ? [...s.messages, { role: 'system' as const, content: t('test.sessionRenewed') }]
            : s.messages
          return { ...s, conversationId: serverConvId, messages: msgs }
        }))
      }

      if (resp.body) {
        const reader = resp.body.getReader()
        convReadersRef.current.set(sid, reader)
        try {
          for await (const { eventType, data } of iterSseLines(reader)) {
            if (data === '[DONE]') break
            if (eventType === 'error') throw new Error(data)
            try {
              const chunk: OpenAIChunk = JSON.parse(data)
              if (chunk.error?.message) throw new Error(chunk.error.message)
              const delta = chunk.choices?.[0]?.delta
              const content = delta?.content
              if (content) {
                fullText += content
                setConversationSessions((prev) => prev.map((s) =>
                  s.id === sid ? { ...s, streamingText: fullText, mcpToolCall: undefined } : s
                ))
              } else if (delta?.tool_calls) {
                const toolName = delta.tool_calls[0]?.function?.name
                if (toolName) {
                  setConversationSessions((prev) => prev.map((s) =>
                    s.id === sid ? { ...s, mcpToolCall: toolName } : s
                  ))
                }
              }
            } catch (err) {
              if (err instanceof SyntaxError) continue
              throw err
            }
          }
        } finally {
          convReadersRef.current.delete(sid)
        }
      }

      setConversationSessions((prev) => prev.map((s) =>
        s.id === sid
          ? { ...s, messages: [...s.messages, { role: 'assistant', content: fullText, model }], streamingText: '', status: 'idle', mcpToolCall: undefined }
          : s
      ))
      onTurnComplete?.()
    } catch (err) {
      setConversationSessions((prev) => prev.map((s) =>
        s.id === sid
          ? { ...s, messages: [...s.messages, { role: 'assistant', content: fullText, model }], streamingText: '', status: 'error', errorMsg: err instanceof Error ? err.message : t('common.unknownError'), mcpToolCall: undefined }
          : s
      ))
      onTurnComplete?.()
    }
  }, [prompt, images, model, activeConvSessionId, conversationSessions, useApiKey, apiKeyValue, endpoint, providerType, useMcp, activeConvSession, onTurnComplete])

  const handleConversationStop = useCallback(() => {
    if (activeConvSessionId === null) return
    const sid = activeConvSessionId
    convReadersRef.current.get(sid)?.cancel()
    convReadersRef.current.delete(sid)
    setConversationSessions((prev) => prev.map((s) =>
      s.id === sid
        ? { ...s, messages: [...s.messages, { role: 'assistant', content: s.streamingText, model }], streamingText: '', status: 'idle' }
        : s
    ))
  }, [activeConvSessionId, model])

  const handleConvClear = useCallback(() => {
    if (activeConvSessionId === null) return
    setConversationSessions((prev) => prev.map((s) =>
      s.id === activeConvSessionId
        ? { ...s, messages: [], streamingText: '', status: 'idle', errorMsg: '' }
        : s
    ))
  }, [activeConvSessionId])

  // ── Run handler ───────────────────────────────────────────────────────────────
  interface RunParams {
    prompt: string; model: string; providerType: string
    endpoint: Endpoint; useApiKey: boolean; images?: string[]
  }

  const executeRun = useCallback(async (p: RunParams) => {
    if ((!p.prompt.trim() && !(p.images && p.images.length > 0)) || !p.model) return
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
        body = {
          model: p.model,
          prompt: p.prompt.trim(),
          stream: isStreaming,
          ...(p.images && p.images.length > 0 && { images: p.images }),
        }
      } else if (p.endpoint === '/api/chat') {
        body = {
          model: p.model,
          messages: [{
            role: 'user',
            content: p.prompt.trim(),
            ...(p.images && p.images.length > 0 && { images: p.images }),
          }],
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
  }, [runs, apiKeyValue, consumeStream])

  const handleRun = useCallback(() => {
    if (mode === 'conversation') {
      executeConversationTurn()
      return
    }
    executeRun({
      prompt, model, providerType, endpoint, useApiKey,
      images: images.length > 0 ? [...images] : undefined,
    })
  }, [mode, executeConversationTurn, executeRun, prompt, model, providerType, endpoint, useApiKey, images])
  // Note: prompt/model/etc. are deps because they're read directly in single mode (passed to executeRun as RunParams)

  const handleStop = useCallback((runId: number) => {
    const reader = readersRef.current.get(runId)
    if (reader) { reader.cancel(); readersRef.current.delete(runId) }
    dispatch({ type: 'SET_STATUS', id: runId, status: 'done' })
  }, [])

  const handleCloseRun = useCallback((runId: number) => {
    const reader = readersRef.current.get(runId)
    if (reader) { reader.cancel(); readersRef.current.delete(runId) }
    dispatch({ type: 'REMOVE', id: runId })
    setActiveRunId((cur) => {
      if (cur !== runId) return cur
      const remaining = runs.filter((r) => r.id !== runId)
      return remaining.length > 0 ? remaining[remaining.length - 1].id : null
    })
  }, [runs])

  const handleRerun = useCallback((run: Run) => {
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
  }, [executeRun])

  const conversationTokenEstimate = activeConvSession
    ? activeConvSession.messages
        .filter((m) => m.role !== 'system')
        .reduce((acc, m) => acc + Math.ceil(m.content.length / 3.5), 0)
    : 0
  const canRun = isLoggedIn() && (!!prompt.trim() || images.length > 0) && !!model &&
    (mode === 'single' || activeConvSession?.status !== 'streaming')
  const isAnyStreaming = runs.some((r) => r.status === 'streaming')
  const isFormStreaming = mode === 'conversation'
    ? activeConvSession?.status === 'streaming'
    : isAnyStreaming

  const handleFormStop = useCallback(() => {
    if (mode === 'conversation') {
      handleConversationStop()
    } else if (activeRunId !== null) {
      handleStop(activeRunId)
    }
  }, [mode, handleConversationStop, handleStop, activeRunId])

  return (
    <Card>
      <CardContent className="p-5 space-y-0">
        <ApiTestForm
          mode={mode}
          providerType={providerType}
          model={model}
          prompt={prompt}
          images={images}
          maxImages={maxImages}
          isCompressing={isCompressing}
          conversationTokenEstimate={conversationTokenEstimate}
          modelContextWindows={modelContextWindows}
          availableOptions={availableOptions}
          availableModels={availableModels}
          isGeminiProvider={isGeminiProvider}
          canRun={canRun}
          authUsername={authUser?.username ?? null}
          endpoint={endpoint}
          useApiKey={useApiKey}
          apiKeyValue={apiKeyValue}
          onModeChange={setMode}
          onProviderChange={setProviderType}
          onModelChange={setModel}
          onPromptChange={setPrompt}
          onImageAdd={handleImageAdd}
          onImageRemove={handleImageRemove}
          onEndpointChange={setEndpoint}
          onUseApiKeyChange={setUseApiKey}
          onApiKeyValueChange={setApiKeyValue}
          isStreaming={!!isFormStreaming}
          onRun={handleRun}
          onStop={handleFormStop}
        />

        {mode === 'conversation' ? (
          <ApiTestConversation
            sessions={conversationSessions}
            activeSessionId={activeConvSessionId}
            messages={activeConvSession?.messages ?? EMPTY_MESSAGES}
            streamingText={activeConvSession?.streamingText ?? ''}
            status={activeConvSession?.status ?? 'idle'}
            errorMsg={activeConvSession?.errorMsg ?? ''}
            mcpToolCall={activeConvSession?.mcpToolCall}
            prompt={prompt}
            images={images}
            maxImages={maxImages}
            isCompressing={isCompressing}
            isGeminiProvider={isGeminiProvider}
            canRun={canRun}
            useMcp={useMcp}
            onUseMcpChange={setUseMcp}
            onNewSession={handleNewConvSession}
            onCloseSession={handleCloseConvSession}
            onSelectSession={setActiveConvSessionId}
            onPromptChange={setPrompt}
            onImageAdd={handleImageAdd}
            onImageRemove={handleImageRemove}
            onRun={handleRun}
            onClear={handleConvClear}
            onStop={handleConversationStop}
          />
        ) : (
          <ApiTestRuns
            runs={runs}
            activeRunId={activeRunId}
            isAnyStreaming={isAnyStreaming}
            onSelectRun={setActiveRunId}
            onCloseRun={handleCloseRun}
            onStop={handleStop}
            onRerun={handleRerun}
          />
        )}
      </CardContent>
    </Card>
  )
}
