'use client'

import { useEffect, useRef, useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { flowJobsQuery } from '@/lib/queries/flow'
import type { Backend, GpuServer, Job } from '@/lib/types'

export interface FlowEvent {
  /** Unique per-event: jobId + phase + spawn timestamp */
  id: string
  jobId: string
  provider: 'ollama' | 'gemini' | string
  backendName: string
  /** Linked GPU server name — null for Gemini or unlinked Ollama */
  serverName: string | null
  model: string
  status: Job['status']
  latencyMs: number | null
  /** Unix ms when this event was detected */
  ts: number
  /**
   * enqueue  = job placed in Valkey queue (API → Queue)
   * dispatch = job dequeued, dispatched to Provider → GPU Server (pending → running)
   * response = inference complete, result returned (running → completed | failed)
   */
  phase: 'enqueue' | 'dispatch' | 'response'
}

/**
 * Polls /v1/dashboard/jobs every 5 s and emits FlowEvents:
 *  - 'enqueue'  on new job detection (job placed in Valkey queue)
 *  - 'dispatch' when a job transitions pending → running (sent to provider/server)
 *  - 'response' when a job transitions pending|running → completed|failed
 *
 * First mount init:
 *  - pending jobs → enqueue (still waiting in queue)
 *  - running jobs → dispatch (currently being processed)
 *
 * Returns a rolling list of the 50 most recent events (newest first).
 */
export function useInferenceStream(
  backends: Backend[],
  servers: GpuServer[],
): FlowEvent[] {
  const statusMap   = useRef<Map<string, Job['status']>>(new Map())
  const initialized = useRef(false)
  const [events, setEvents] = useState<FlowEvent[]>([])

  /** backend name → { provider, serverName } */
  const backendMeta = useMemo(() => {
    const serverById = new Map(servers.map(s => [s.id, s.name]))
    return new Map(
      backends.map(b => [
        b.name,
        {
          provider:   b.backend_type as 'ollama' | 'gemini',
          serverName: b.server_id ? (serverById.get(b.server_id) ?? null) : null,
        },
      ]),
    )
  }, [backends, servers])

  const { data } = useQuery(flowJobsQuery)

  useEffect(() => {
    if (!data) return
    const jobs = data.jobs
    const now  = Date.now()

    if (!initialized.current) {
      // First mount: snapshot all statuses.
      // Emit enqueue for pending (in queue) and dispatch for running (being processed).
      const initEvents: FlowEvent[] = []
      jobs.forEach(j => {
        statusMap.current.set(j.id, j.status)
        const meta = backendMeta.get(j.backend)
        const base = {
          jobId: j.id, provider: meta?.provider ?? 'ollama', backendName: j.backend,
          serverName: meta?.serverName ?? null, model: j.model_name,
          status: j.status, latencyMs: j.latency_ms, ts: now,
        }
        if (j.status === 'pending') {
          initEvents.push({ id: `${j.id}-init-${now}`, ...base, phase: 'enqueue' })
        } else if (j.status === 'running') {
          initEvents.push({ id: `${j.id}-init-${now}`, ...base, phase: 'dispatch' })
        }
      })
      initialized.current = true
      if (initEvents.length > 0) setEvents(initEvents)
      return
    }

    const newEvents: FlowEvent[] = []

    for (const j of jobs) {
      const prevStatus = statusMap.current.get(j.id)
      const meta = backendMeta.get(j.backend)
      const base = {
        jobId: j.id, provider: meta?.provider ?? 'ollama', backendName: j.backend,
        serverName: meta?.serverName ?? null, model: j.model_name,
        status: j.status, latencyMs: j.latency_ms, ts: now,
      }

      if (prevStatus === undefined) {
        // First time seeing this job — emit based on current status
        statusMap.current.set(j.id, j.status)
        if (j.status === 'running') {
          // Missed pending state — show dispatch (already past enqueue)
          newEvents.push({ id: `${j.id}-dsp-${now}`, ...base, phase: 'dispatch' })
        } else if (j.status === 'completed' || j.status === 'failed' || j.status === 'cancelled') {
          // Missed entire pipeline — show response
          newEvents.push({ id: `${j.id}-res-${now}`, ...base, phase: 'response' })
        } else {
          // pending or unknown — enqueue
          newEvents.push({ id: `${j.id}-enq-${now}`, ...base, phase: 'enqueue' })
        }
      } else if (prevStatus === 'pending' && j.status === 'running') {
        // Dispatched to provider/server
        statusMap.current.set(j.id, j.status)
        newEvents.push({ id: `${j.id}-dsp-${now}`, ...base, phase: 'dispatch' })
      } else if (
        (prevStatus === 'pending' || prevStatus === 'running') &&
        (j.status === 'completed' || j.status === 'failed' || j.status === 'cancelled')
      ) {
        // Inference finished or cancelled — response animation (bypasses queue on return)
        statusMap.current.set(j.id, j.status)
        newEvents.push({ id: `${j.id}-res-${now}`, ...base, phase: 'response' })
      } else if (prevStatus !== j.status) {
        // Any other status change — update map only, no animation
        statusMap.current.set(j.id, j.status)
      }
    }

    if (newEvents.length > 0) {
      setEvents(prev => [...newEvents, ...prev].slice(0, 50))
    }
  }, [data, backendMeta])

  return events
}
