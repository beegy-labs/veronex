'use client'

/**
 * Provider Flow Panel
 *
 * 3-phase bidirectional flow animation:
 *   Enqueue  (API → Queue):             new job enters Valkey queue
 *   Dispatch (Queue → Provider → GPU):  job dequeued, sent to backend
 *   Response (GPU → Provider → API):    result returned, bypasses Queue
 *
 * Provider topologies:
 *   Ollama + server:  Queue → Ollama → GPU Server  |  GPU Server → Ollama → API
 *   Ollama only:      Queue → Ollama               |  Ollama → API
 *   Gemini:           Queue → Gemini               |  Gemini → API
 *
 * Hops are staggered by BEE_STAGGER_MS so each segment fires sequentially.
 * Animation: CSS offset-path + @keyframes bee-fly (GPU-composited, not SMIL)
 * State:     useReducer (SPAWN / EXPIRE) — stable dispatch ref
 * Cleanup:   onAnimationEnd — no setTimeout leaks
 * Scaling:   ResizeObserver → transform:scale on bee overlay
 */

import { useEffect, useRef, useReducer, useMemo } from 'react'
import { useTranslation } from '@/i18n'
import type { Backend, GpuServer } from '@/lib/types'
import type { FlowEvent } from '@/hooks/use-inference-stream'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

/* ─── constants ─────────────────────────────────────────────── */
const VIEW_W = 450
const VIEW_H = 260
const MAX_BEES = 30

// Bee animation duration must match globals.css @keyframes bee-fly (1400 ms).
const BEE_DURATION_MS = 1400
const BEE_STAGGER_MS  = Math.floor(BEE_DURATION_MS / 2)  // 700 ms

// Enqueue bees are always bright yellow — visually "requests clustering toward the queue"
const ENQUEUE_COLOR = '#facc15'  // yellow-400

// Column 1: Veronex API
const API_CX    = 56
const API_W     = 96
const API_H     = 36
const API_CY    = 140   // midpoint between Ollama (80) and Gemini (200)
const API_RIGHT = API_CX + API_W / 2   // 104

// Column 2: Queue (Valkey)
const QUEUE_CX    = 172
const QUEUE_W     = 72
const QUEUE_H     = 36
const QUEUE_CY    = 140
const QUEUE_LEFT  = QUEUE_CX - QUEUE_W / 2   // 136
const QUEUE_RIGHT = QUEUE_CX + QUEUE_W / 2   // 208

// Column 3: Providers
const PROV_CX    = 288
const PROV_W     = 96
const PROV_H     = 36
const OLLAMA_CY  = 80
const GEMINI_CY  = 200
const PROV_LEFT  = PROV_CX - PROV_W / 2   // 240
const PROV_RIGHT = PROV_CX + PROV_W / 2   // 336

// Column 4: GPU servers (Ollama only)
const GPU_CX      = 404
const GPU_W       = 72
const GPU_H       = 28
const GPU_LEFT    = GPU_CX - GPU_W / 2   // 368
const GPU_SPACING = 30

/* ─── paths: enqueue (API → Queue) ──────────────────────────── */
const PATH_API_QUEUE =
  `M ${API_RIGHT},${API_CY} C ${API_RIGHT + 10},${API_CY} ${QUEUE_LEFT - 10},${QUEUE_CY} ${QUEUE_LEFT},${QUEUE_CY}`

/* ─── paths: dispatch (Queue → Provider → Server) ───────────── */
const PATH_QUEUE_OLLAMA =
  `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 16},${QUEUE_CY} ${PROV_LEFT - 16},${OLLAMA_CY} ${PROV_LEFT},${OLLAMA_CY}`
const PATH_QUEUE_GEMINI =
  `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 16},${QUEUE_CY} ${PROV_LEFT - 16},${GEMINI_CY} ${PROV_LEFT},${GEMINI_CY}`

function pathOllamaToServer(serverCy: number): string {
  return `M ${PROV_RIGHT},${OLLAMA_CY} C ${PROV_RIGHT + 24},${OLLAMA_CY} ${GPU_LEFT - 24},${serverCy} ${GPU_LEFT},${serverCy}`
}

/* ─── paths: response — bypass Queue (arc above/below) ──────── */
// Ollama → API: arcs above Queue node (Queue cy=140, arc passes at y≈61)
const PATH_OLLAMA_API =
  `M ${PROV_LEFT},${OLLAMA_CY} C ${PROV_LEFT - 44},${OLLAMA_CY - 35} ${API_RIGHT + 44},${OLLAMA_CY - 35} ${API_RIGHT},${API_CY}`
// Gemini → API: arcs below Queue node (Queue cy=140, arc passes at y≈219)
const PATH_GEMINI_API =
  `M ${PROV_LEFT},${GEMINI_CY} C ${PROV_LEFT - 44},${GEMINI_CY + 35} ${API_RIGHT + 44},${GEMINI_CY + 35} ${API_RIGHT},${API_CY}`

function pathServerToOllama(serverCy: number): string {
  return `M ${GPU_LEFT},${serverCy} C ${GPU_LEFT - 24},${serverCy} ${PROV_RIGHT + 24},${OLLAMA_CY} ${PROV_RIGHT},${OLLAMA_CY}`
}

function serverNodeY(i: number, total: number): number {
  const totalSpan = (total - 1) * GPU_SPACING
  return OLLAMA_CY - totalSpan / 2 + i * GPU_SPACING
}

/* ─── helpers ────────────────────────────────────────────────── */
function statusColor(status: string): string {
  switch (status) {
    case 'completed': return 'var(--theme-status-success)'
    case 'failed':    return 'var(--theme-status-error)'
    case 'running':   return 'var(--theme-status-info)'
    case 'cancelled': return 'var(--theme-status-cancelled)'
    default:          return 'var(--theme-status-warning)'
  }
}

function nodeStroke(backends: Backend[]): string {
  if (backends.length === 0)                        return 'var(--theme-border)'
  if (backends.some(b => b.status === 'online'))    return 'var(--theme-status-success)'
  if (backends.some(b => b.status === 'degraded'))  return 'var(--theme-status-warning)'
  return 'var(--theme-status-error)'
}

/* ─── bee reducer ────────────────────────────────────────────── */
type Bee = {
  id: string
  pathD: string
  color: string
  phase: 'enqueue' | 'dispatch' | 'response'
  delay: number
}
type Action = { type: 'SPAWN'; bees: Bee[] } | { type: 'EXPIRE'; id: string }

function beeReducer(state: Bee[], action: Action): Bee[] {
  switch (action.type) {
    case 'SPAWN':  return [...state, ...action.bees].slice(-MAX_BEES)
    case 'EXPIRE': return state.filter(b => b.id !== action.id)
  }
}

/* ─── SVG sub-components ─────────────────────────────────────── */
function NodeBox({
  cx, cy, w = 96, h = 36, label, sublabel, stroke,
}: {
  cx: number; cy: number; w?: number; h?: number
  label: string; sublabel?: string; stroke: string
}) {
  return (
    <g>
      <rect
        x={cx - w / 2} y={cy - h / 2} width={w} height={h} rx="8"
        style={{ fill: 'var(--theme-bg-card)', stroke, strokeWidth: 1.5 }}
      />
      <text x={cx} y={sublabel ? cy - 4 : cy + 5} textAnchor="middle"
        style={{ fill: 'var(--theme-text-primary)', fontSize: 11, fontWeight: 600 }}>
        {label}
      </text>
      {sublabel && (
        <text x={cx} y={cy + 9} textAnchor="middle"
          style={{ fill: 'var(--theme-text-secondary)', fontSize: 8 }}>
          {sublabel}
        </text>
      )}
    </g>
  )
}

function GpuNode({ cy, label, active }: { cy: number; label: string; active: boolean }) {
  const truncated = label.length > 9 ? label.slice(0, 8) + '…' : label
  return (
    <g>
      <rect
        x={GPU_LEFT} y={cy - GPU_H / 2} width={GPU_W} height={GPU_H} rx="5"
        style={{
          fill: 'var(--theme-bg-card)',
          stroke: active ? 'var(--theme-status-info)' : 'var(--theme-border)',
          strokeWidth: active ? 1.5 : 1,
        }}
      />
      <text x={GPU_CX} y={cy + 4} textAnchor="middle"
        style={{ fill: 'var(--theme-text-primary)', fontSize: 9, fontWeight: 600 }}>
        {truncated}
      </text>
      {active && (
        <circle
          cx={GPU_LEFT + GPU_W - 6} cy={cy - GPU_H / 2 + 6} r={3}
          style={{ fill: 'var(--theme-status-info)' }}
        />
      )}
    </g>
  )
}

/* ─── panel ──────────────────────────────────────────────────── */
interface Props {
  backends: Backend[]
  servers: GpuServer[]
  events: FlowEvent[]
}

export function ProviderFlowPanel({ backends, servers, events }: Props) {
  const { t } = useTranslation()
  const spawnedRef   = useRef(new Set<string>())
  const containerRef = useRef<HTMLDivElement>(null)
  const [scale,  setScale]  = useReducer((_: number, v: number) => v, 1)
  const [bees,   dispatch]  = useReducer(beeReducer, [])

  const localBs = useMemo(() => backends.filter(b => b.backend_type === 'ollama'), [backends])
  const apiBs   = useMemo(() => backends.filter(b => b.backend_type === 'gemini'), [backends])

  // GPU servers linked to ≥1 Ollama backend (max 5 shown)
  const linkedServerIds = useMemo(() => new Set(
    localBs.filter(b => b.server_id).map(b => b.server_id!),
  ), [localBs])

  const displayServers = useMemo(
    () => servers.filter(s => linkedServerIds.has(s.id)).slice(0, 5),
    [servers, linkedServerIds],
  )

  // serverName → SVG y-coordinate (right column)
  const serverYMap = useMemo(() => {
    const total = displayServers.length
    return new Map(displayServers.map((s, i) => [s.name, serverNodeY(i, total)]))
  }, [displayServers])

  // Responsive scaling
  useEffect(() => {
    if (!containerRef.current) return
    const obs = new ResizeObserver(([entry]) => {
      setScale(entry.contentRect.width / VIEW_W)
    })
    obs.observe(containerRef.current)
    return () => obs.disconnect()
  }, [])

  // Spawn bees for new events — 3-phase bidirectional
  useEffect(() => {
    const newEvs = events.filter(e => !spawnedRef.current.has(e.id))
    if (newEvs.length === 0) return
    newEvs.forEach(e => spawnedRef.current.add(e.id))

    const newBees: Bee[] = []
    for (const e of newEvs) {
      const color = statusColor(e.status)

      if (e.phase === 'enqueue') {
        // ── API → Queue (bright yellow — requests clustering toward queue) ─
        newBees.push({ id: `${e.id}-eq`, pathD: PATH_API_QUEUE, color: ENQUEUE_COLOR, phase: 'enqueue', delay: 0 })

      } else if (e.phase === 'dispatch') {
        // ── Queue → Provider [→ Server] ──────────────────────
        if (e.provider === 'gemini') {
          newBees.push({ id: `${e.id}-qg`, pathD: PATH_QUEUE_GEMINI, color, phase: 'dispatch', delay: 0 })
        } else {
          // Hop 1: Queue → Ollama
          newBees.push({ id: `${e.id}-qo`, pathD: PATH_QUEUE_OLLAMA, color, phase: 'dispatch', delay: 0 })
          // Hop 2: Ollama → GPU Server (staggered)
          if (e.serverName && serverYMap.has(e.serverName)) {
            newBees.push({
              id: `${e.id}-os`, pathD: pathOllamaToServer(serverYMap.get(e.serverName)!),
              color, phase: 'dispatch', delay: BEE_STAGGER_MS,
            })
          }
        }

      } else {
        // ── Response: Server [→ Ollama] → API (bypasses Queue) ─
        if (e.provider === 'gemini') {
          newBees.push({ id: `${e.id}-ga`, pathD: PATH_GEMINI_API, color, phase: 'response', delay: 0 })
        } else {
          if (e.serverName && serverYMap.has(e.serverName)) {
            // Hop 1: GPU Server → Ollama
            newBees.push({
              id: `${e.id}-so`, pathD: pathServerToOllama(serverYMap.get(e.serverName)!),
              color, phase: 'response', delay: 0,
            })
            // Hop 2: Ollama → API (bypasses Queue, staggered)
            newBees.push({
              id: `${e.id}-oa`, pathD: PATH_OLLAMA_API,
              color, phase: 'response', delay: BEE_STAGGER_MS,
            })
          } else {
            // No server: single hop Ollama → API
            newBees.push({ id: `${e.id}-oa`, pathD: PATH_OLLAMA_API, color, phase: 'response', delay: 0 })
          }
        }
      }
    }

    if (newBees.length > 0) dispatch({ type: 'SPAWN', bees: newBees })
  }, [events, serverYMap])

  // 5-min enqueue counts (new job arrivals per provider)
  const ollamaCount = useMemo(() => {
    const cutoff = Date.now() - 5 * 60_000
    return events.filter(e => e.phase === 'enqueue' && e.provider !== 'gemini' && e.ts > cutoff).length
  }, [events])

  const geminiCount = useMemo(() => {
    const cutoff = Date.now() - 5 * 60_000
    return events.filter(e => e.phase === 'enqueue' && e.provider === 'gemini' && e.ts > cutoff).length
  }, [events])

  // Active servers: GPU servers with dispatch events in last 5 m
  const activeServers = useMemo(() => {
    const cutoff = Date.now() - 5 * 60_000
    return new Set(
      events
        .filter(e => e.ts > cutoff && e.phase === 'dispatch' && e.serverName)
        .map(e => e.serverName!),
    )
  }, [events])

  const extraServers = servers.filter(s => linkedServerIds.has(s.id)).length - 5

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold">{t('overview.providerFlow')}</CardTitle>
      </CardHeader>
      <CardContent className="p-0 pb-2">
        <div
          ref={containerRef}
          className="relative w-full overflow-hidden"
          style={{ aspectRatio: `${VIEW_W} / ${VIEW_H}` }}
        >
          {/* SVG topology */}
          <svg
            viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
            className="absolute inset-0 w-full h-full"
            aria-label="Provider flow topology"
          >
            {/* Dispatch path guides: API → Queue → Providers → Servers */}
            <path d={PATH_API_QUEUE} fill="none"
              style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '5 4' }} />
            <path d={PATH_QUEUE_OLLAMA} fill="none"
              style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '5 4' }} />
            <path d={PATH_QUEUE_GEMINI} fill="none"
              style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '5 4' }} />
            {displayServers.map(s => (
              <path
                key={s.id}
                d={pathOllamaToServer(serverYMap.get(s.name)!)}
                fill="none"
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '5 4' }}
              />
            ))}

            {/* Response bypass arc guides: Provider → API (bypasses Queue) */}
            <path d={PATH_OLLAMA_API} fill="none"
              style={{ stroke: 'var(--theme-border)', strokeWidth: 1, strokeDasharray: '3 6', opacity: 0.5 }} />
            <path d={PATH_GEMINI_API} fill="none"
              style={{ stroke: 'var(--theme-border)', strokeWidth: 1, strokeDasharray: '3 6', opacity: 0.5 }} />

            {/* Column 1: Veronex API */}
            <NodeBox
              cx={API_CX} cy={API_CY}
              w={API_W} h={API_H}
              label="Veronex API"
              stroke="var(--theme-primary)"
            />

            {/* Column 2: Queue (Valkey) */}
            <NodeBox
              cx={QUEUE_CX} cy={QUEUE_CY}
              w={QUEUE_W} h={QUEUE_H}
              label="Queue"
              sublabel="Valkey"
              stroke="var(--theme-border)"
            />

            {/* Column 3: Ollama */}
            <NodeBox
              cx={PROV_CX} cy={OLLAMA_CY}
              w={PROV_W} h={PROV_H}
              label="Ollama"
              stroke={nodeStroke(localBs)}
              sublabel={localBs.length > 0
                ? `${localBs.filter(b => b.status === 'online').length}/${localBs.length} online`
                : 'no backends'}
            />

            {/* Column 3: Gemini */}
            <NodeBox
              cx={PROV_CX} cy={GEMINI_CY}
              w={PROV_W} h={PROV_H}
              label="Gemini"
              stroke={nodeStroke(apiBs)}
              sublabel={apiBs.length > 0
                ? `${apiBs.filter(b => b.status === 'online').length}/${apiBs.length} online`
                : 'no backends'}
            />

            {/* Column 4: GPU server nodes */}
            {displayServers.map(s => (
              <GpuNode
                key={s.id}
                cy={serverYMap.get(s.name)!}
                label={s.name}
                active={activeServers.has(s.name)}
              />
            ))}

            {/* +N more indicator */}
            {extraServers > 0 && (
              <text
                x={GPU_CX}
                y={serverNodeY(4, 5) + GPU_H / 2 + 14}
                textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 8 }}
              >
                +{extraServers} more
              </text>
            )}

            {/* 5-min enqueue counts (job arrivals per provider) */}
            {ollamaCount > 0 && (
              <text x={PROV_CX} y={OLLAMA_CY + PROV_H / 2 + 12} textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 9 }}>
                {t('overview.reqLast5m', { count: ollamaCount })}
              </text>
            )}
            {geminiCount > 0 && (
              <text x={PROV_CX} y={GEMINI_CY + PROV_H / 2 + 12} textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 9 }}>
                {t('overview.reqLast5m', { count: geminiCount })}
              </text>
            )}
          </svg>

          {/* Bee overlay — fixed VIEW_W × VIEW_H logical space, scaled to match SVG */}
          <div
            className="absolute top-0 left-0 pointer-events-none"
            style={{
              width: VIEW_W,
              height: VIEW_H,
              transform: `scale(${scale})`,
              transformOrigin: 'top left',
            }}
          >
            {bees.map(bee => (
              <div
                key={bee.id}
                className="bee-particle"
                style={{
                  offsetPath:      `path("${bee.pathD}")`,
                  backgroundColor: bee.phase === 'response' ? `${bee.color}cc` : bee.color,
                  boxShadow:       `0 0 6px 2px ${bee.color}${bee.phase === 'response' ? '28' : '44'}`,
                  animationDelay:  bee.delay > 0 ? `${bee.delay}ms` : undefined,
                }}
                onAnimationEnd={() => dispatch({ type: 'EXPIRE', id: bee.id })}
              />
            ))}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
