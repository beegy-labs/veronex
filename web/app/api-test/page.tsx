'use client'

import { useState, useRef, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { Send, Loader2, X } from 'lucide-react'

export default function ApiTestPage() {
  const [prompt, setPrompt] = useState('')
  const [model, setModel] = useState('')
  const [backend, setBackend] = useState('ollama')

  // Fetch registered backends
  const { data: backends } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    staleTime: 60_000,
  })

  // The selected backend ID for model lookup (first active backend of chosen type)
  const selectedBackend = backends?.find(
    (b) => b.backend_type === backend && b.is_active
  )

  // Fetch models for the selected backend
  const { data: modelsData } = useQuery({
    queryKey: ['backend-models', selectedBackend?.id],
    queryFn: () => api.backendModels(selectedBackend!.id),
    enabled: !!selectedBackend,
    staleTime: 60_000,
  })

  const availableModels = modelsData?.models ?? []
  const availableBackendTypes = [...new Set(backends?.filter((b) => b.is_active).map((b) => b.backend_type) ?? ['ollama', 'gemini'])]

  // Reset model when backend type or model list changes
  useEffect(() => {
    if (availableModels.length > 0 && !availableModels.includes(model)) {
      setModel(availableModels[0])
    }
  }, [availableModels, model])
  const [tokens, setTokens] = useState<string[]>([])
  const [status, setStatus] = useState<'idle' | 'submitting' | 'streaming' | 'done' | 'error'>('idle')
  const [errorMsg, setErrorMsg] = useState('')
  const [jobId, setJobId] = useState<string | null>(null)
  const esRef = useRef<EventSource | null>(null)

  const BASE = process.env.NEXT_PUBLIC_INFERQ_API_URL ?? 'http://localhost:3001'
  const KEY  = process.env.NEXT_PUBLIC_INFERQ_ADMIN_KEY ?? ''

  function stopStream() {
    if (esRef.current) {
      esRef.current.close()
      esRef.current = null
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!prompt.trim() || status === 'submitting' || status === 'streaming') return

    stopStream()
    setTokens([])
    setErrorMsg('')
    setJobId(null)
    setStatus('submitting')

    try {
      const resp = await api.submitInference({ prompt: prompt.trim(), model, backend })
      setJobId(resp.job_id)
      setStatus('streaming')

      // Open SSE stream
      const url = `${BASE}/v1/inference/${resp.job_id}/stream`
      const es = new EventSource(url, {
        // EventSource doesn't support custom headers natively;
        // we append the key as a query parameter as a fallback.
        // In production, use a proxy or cookie-based auth.
      } as EventSourceInit & { headers?: Record<string, string> })

      // Workaround: use fetch+ReadableStream for header support
      es.close()

      // Use fetch-based SSE instead so we can send the X-API-Key header
      const streamResponse = await fetch(url, {
        headers: { 'X-API-Key': KEY },
      })

      if (!streamResponse.ok || !streamResponse.body) {
        throw new Error(`Stream failed: ${streamResponse.status} ${streamResponse.statusText}`)
      }

      const reader = streamResponse.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''

      setStatus('streaming')

      // eslint-disable-next-line no-constant-condition
      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() ?? ''

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const raw = line.slice(6).trim()
            if (raw === '[DONE]') {
              setStatus('done')
              reader.cancel()
              return
            }
            try {
              const parsed = JSON.parse(raw) as { token?: string; done?: boolean }
              if (parsed.token != null) {
                setTokens((prev) => [...prev, parsed.token!])
              }
              if (parsed.done) {
                setStatus('done')
                reader.cancel()
                return
              }
            } catch {
              // ignore malformed SSE data
            }
          }
        }
      }

      setStatus('done')
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : 'Unknown error')
      setStatus('error')
    }
  }

  function handleReset() {
    stopStream()
    setTokens([])
    setErrorMsg('')
    setJobId(null)
    setStatus('idle')
  }

  const output = tokens.join('')
  const isRunning = status === 'submitting' || status === 'streaming'

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <h1 className="text-2xl font-bold text-slate-100">Inference Test</h1>
        <p className="text-slate-400 mt-1 text-sm">
          Submit a prompt and stream the response in real-time.
        </p>
      </div>

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Model + Backend */}
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Backend</label>
            <select
              value={backend}
              onChange={(e) => { setBackend(e.target.value); setModel('') }}
              disabled={isRunning}
              className="w-full bg-slate-800 border border-slate-700 text-slate-200 text-sm rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-indigo-500 disabled:opacity-50"
            >
              {availableBackendTypes.map((b) => (
                <option key={b} value={b}>{b}</option>
              ))}
            </select>
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Model</label>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={isRunning || availableModels.length === 0}
              className="w-full bg-slate-800 border border-slate-700 text-slate-200 text-sm rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-indigo-500 disabled:opacity-50"
            >
              {availableModels.length === 0 && (
                <option value="">No models available</option>
              )}
              {availableModels.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
          </div>
        </div>

        {/* Prompt */}
        <div>
          <label className="block text-sm font-medium text-slate-300 mb-1">Prompt</label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            disabled={isRunning}
            rows={4}
            placeholder="Enter your prompt here…"
            className="w-full bg-slate-800 border border-slate-700 text-slate-200 text-sm rounded-lg px-3 py-2 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 resize-y disabled:opacity-50"
          />
        </div>

        {/* Submit button */}
        <div className="flex gap-3">
          <button
            type="submit"
            disabled={!prompt.trim() || isRunning}
            className="flex items-center gap-2 px-5 py-2.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
          >
            {isRunning ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Send className="h-4 w-4" />
            )}
            {status === 'submitting' ? 'Submitting…' : status === 'streaming' ? 'Streaming…' : 'Run'}
          </button>

          {(status !== 'idle') && (
            <button
              type="button"
              onClick={handleReset}
              className="flex items-center gap-2 px-4 py-2.5 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm transition-colors"
            >
              <X className="h-4 w-4" />
              Reset
            </button>
          )}
        </div>
      </form>

      {/* Job ID info */}
      {jobId && (
        <p className="text-xs text-slate-500 font-mono">Job ID: {jobId}</p>
      )}

      {/* Output */}
      {(output || status === 'streaming' || status === 'done') && (
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-5">
          <div className="flex items-center justify-between mb-3">
            <p className="text-xs font-medium text-slate-400 uppercase tracking-wider">Output</p>
            {status === 'streaming' && (
              <span className="flex items-center gap-1.5 text-xs text-blue-400">
                <span className="h-1.5 w-1.5 rounded-full bg-blue-400 animate-pulse" />
                Streaming
              </span>
            )}
            {status === 'done' && (
              <span className="text-xs text-emerald-400">Complete</span>
            )}
          </div>
          <pre className="text-sm text-slate-200 whitespace-pre-wrap font-mono leading-relaxed min-h-[2rem]">
            {output}
            {status === 'streaming' && (
              <span className="inline-block w-0.5 h-4 bg-slate-400 animate-pulse ml-px align-middle" />
            )}
          </pre>
        </div>
      )}

      {/* Error */}
      {status === 'error' && (
        <div className="rounded-xl border border-red-800 bg-red-950 p-5 text-red-300">
          <p className="font-semibold">Error</p>
          <p className="text-sm mt-1 text-red-400">{errorMsg}</p>
        </div>
      )}
    </div>
  )
}
