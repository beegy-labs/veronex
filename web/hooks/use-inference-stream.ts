'use client'

import { useEffect, useRef, useState, useMemo } from 'react'
import { getAccessToken } from '@/lib/auth'
import type { Backend } from '@/lib/types'

const BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

export interface FlowEvent {
  /** Unique per-event: jobId + phase + spawn timestamp */
  id: string
  jobId: string
  provider: 'ollama' | 'gemini' | string
  backendName: string
  model: string
  status: string
  latencyMs: number | null
  /** Unix ms when this event was detected */
  ts: number
  /**
   * enqueue  = job placed in Valkey queue (API → Queue)
   * dispatch = job sent to Provider (pending → running)
   * response = inference complete (running → completed | failed)
   */
  phase: 'enqueue' | 'dispatch' | 'response'
}

/** Raw shape of JobStatusEvent from the SSE stream */
interface RawJobStatusEvent {
  id: string
  status: string
  model_name: string
  backend: string
  latency_ms: number | null
}

/**
 * Connects to /v1/dashboard/jobs/stream (SSE) and emits FlowEvents in real time.
 * Each SSE event carries a single job status transition — no polling needed.
 *
 * Phase mapping:
 *  pending  → enqueue  (job entered Valkey queue)
 *  running  → dispatch (job sent to provider)
 *  completed | failed | cancelled → response
 *
 * Returns a rolling list of the 50 most recent events (newest first).
 */
export function useInferenceStream(backends: Backend[]): FlowEvent[] {
  const [events, setEvents] = useState<FlowEvent[]>([])

  /** backend name → provider type */
  const backendTypeMap = useMemo(
    () => new Map(backends.map(b => [b.name, b.backend_type as 'ollama' | 'gemini'])),
    [backends],
  )

  const backendTypeMapRef = useRef(backendTypeMap)
  useEffect(() => { backendTypeMapRef.current = backendTypeMap }, [backendTypeMap])

  useEffect(() => {
    let active = true
    let retryTimer: ReturnType<typeof setTimeout> | null = null
    let retryDelay = 2_000

    function connect() {
      const token = getAccessToken()
      if (!token) return

      const ctrl = new AbortController()

      fetch(`${BASE}/v1/dashboard/jobs/stream`, {
        headers: { Authorization: `Bearer ${token}` },
        signal: ctrl.signal,
      })
        .then(async res => {
          if (!res.ok || !res.body) throw new Error(`SSE ${res.status}`)
          retryDelay = 2_000 // reset backoff on success

          const reader = res.body.getReader()
          const decoder = new TextDecoder()
          let buf = ''

          while (active) {
            const { value, done } = await reader.read()
            if (done) break
            buf += decoder.decode(value, { stream: true })

            // SSE frames end with '\n\n'
            const frames = buf.split('\n\n')
            buf = frames.pop() ?? ''

            for (const frame of frames) {
              // Parse 'data: {...}' lines
              const dataLine = frame.split('\n').find(l => l.startsWith('data:'))
              if (!dataLine) continue
              try {
                const raw: RawJobStatusEvent = JSON.parse(dataLine.slice(5).trim())
                const phase =
                  raw.status === 'pending'
                    ? 'enqueue'
                    : raw.status === 'running'
                      ? 'dispatch'
                      : 'response'

                const provider =
                  backendTypeMapRef.current.get(raw.backend) ?? 'ollama'

                const event: FlowEvent = {
                  id: `${raw.id}-${raw.status}-${Date.now()}`,
                  jobId: raw.id,
                  provider,
                  backendName: raw.backend,
                  model: raw.model_name,
                  status: raw.status,
                  latencyMs: raw.latency_ms,
                  ts: Date.now(),
                  phase,
                }

                if (active) {
                  setEvents(prev => [event, ...prev].slice(0, 50))
                }
              } catch {
                // malformed JSON — skip
              }
            }
          }
        })
        .catch(err => {
          if (!active) return
          if ((err as Error).name === 'AbortError') return
          // Reconnect with exponential backoff (max 30 s)
          retryTimer = setTimeout(() => {
            if (active) {
              retryDelay = Math.min(retryDelay * 2, 30_000)
              connect()
            }
          }, retryDelay)
        })

      return ctrl
    }

    const ctrl = connect()

    return () => {
      active = false
      ctrl?.abort()
      if (retryTimer) clearTimeout(retryTimer)
    }
  }, []) // connect once on mount; backendTypeMapRef stays current via ref

  return events
}
