'use client'

import { useEffect, useState } from 'react'
import { isLoggedIn } from '@/lib/auth'
import { BASE_API_URL as BASE } from '@/lib/constants'
import type { FlowStats } from '@/lib/generated'
import { FlowStatsSchema, JobStatusEventSchema } from '@/lib/api-schemas'

export interface FlowEvent {
  /** Unique per-event: jobId + phase + spawn timestamp */
  id: string
  jobId: string
  provider: 'ollama' | 'gemini' | string
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

/** Maximum entries kept in the per-connection deduplication set before it is cleared. */
const SEEN_CAP = 1_000

/**
 * Connects to /v1/dashboard/jobs/stream (SSE) and emits FlowEvents in real time.
 *
 * On connect the server first replays its ring buffer (last 100 events, oldest→newest)
 * then streams live events. This guarantees all users see the same feed regardless
 * of when they connected — late joiners get the same history as long-connected clients.
 *
 * Phase mapping:
 *  pending  → enqueue  (job entered Valkey queue)
 *  running  → dispatch (job sent to provider)
 *  completed | failed | cancelled → response
 *
 * Returns a rolling list of the 100 most recent events (newest first)
 * and the latest server-computed aggregate stats.
 */

export function useInferenceStream(): { events: FlowEvent[]; stats: FlowStats | null } {
  const [events, setEvents] = useState<FlowEvent[]>([])
  const [stats, setStats] = useState<FlowStats | null>(null)

  useEffect(() => {
    let active = true
    let retryTimer: ReturnType<typeof setTimeout> | null = null
    let retryDelay = 2_000

    function connect() {
      if (!isLoggedIn()) return

      const ctrl = new AbortController()

      fetch(`${BASE}/v1/dashboard/jobs/stream`, {
        credentials: 'include',
        signal: ctrl.signal,
      })
        .then(async res => {
          if (!res.ok || !res.body) throw new Error(`SSE ${res.status}`)
          retryDelay = 2_000 // reset backoff on success

          const reader = res.body.getReader()
          const decoder = new TextDecoder()
          let buf = ''

          // Deduplication set: prevents duplicates when a live event arrives
          // within the replay window (jobId:status is the canonical key).
          const seen = new Set<string>()

          while (active) {
            const { value, done } = await reader.read()
            if (done) break
            buf += decoder.decode(value, { stream: true })

            // SSE frames end with '\n\n'
            const frames = buf.split('\n\n')
            buf = frames.pop() ?? ''

            for (const frame of frames) {
              const lines = frame.split('\n')
              const eventType = lines.find(l => l.startsWith('event:'))?.slice(6).trim() ?? 'job_status'
              const dataLine = lines.find(l => l.startsWith('data:'))
              if (!dataLine) continue

              let payload: unknown
              try {
                payload = JSON.parse(dataLine.slice(5).trim())
              } catch {
                continue
              }

              if (eventType === 'flow_stats') {
                const result = FlowStatsSchema.safeParse(payload)
                if (!result.success || !active) continue
                const next = result.data
                setStats(prev =>
                  prev &&
                  prev.incoming  === next.incoming &&
                  prev.queued    === next.queued &&
                  prev.running   === next.running &&
                  prev.completed === next.completed
                    ? prev
                    : next
                )
                continue
              }

              // job_status event
              const result = JobStatusEventSchema.safeParse(payload)
              if (!result.success) continue
              const raw = result.data

              // Deduplicate: same job+status transition = same logical event.
              // Cap size to prevent unbounded growth on long-lived connections.
              // Evict oldest half (insertion-order) rather than full clear to avoid
              // re-processing recently seen events that arrive just after a clear.
              const dedupeKey = `${raw.id}:${raw.status}`
              if (seen.has(dedupeKey)) continue
              if (seen.size >= SEEN_CAP) {
                const iter = seen.values()
                for (let i = 0; i < SEEN_CAP >> 1; i++) {
                  const { value, done } = iter.next()
                  if (done) break
                  seen.delete(value)
                }
              }
              seen.add(dedupeKey)

              const phase =
                raw.status === 'pending'
                  ? 'enqueue'
                  : raw.status === 'running'
                    ? 'dispatch'
                    : 'response'

              const event: FlowEvent = {
                id: `${raw.id}-${raw.status}`,
                jobId: raw.id,
                provider: raw.provider_type,
                model: raw.model_name,
                status: raw.status,
                latencyMs: raw.latency_ms,
                ts: raw.ts ?? Date.now(),
                phase,
              }

              if (active) {
                setEvents(prev => {
                  const next = [event, ...prev]
                  return next.length > 100 ? next.slice(0, 100) : next
                })
              }
            }
          }
        })
        .catch(err => {
          if (!active) return
          if ((err as Error).name === 'AbortError') return
          // Clear stale stats so the UI shows 0 rather than stale values during reconnect
          setStats(null)
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
  }, []) // connect once on mount

  return { events, stats }
}
