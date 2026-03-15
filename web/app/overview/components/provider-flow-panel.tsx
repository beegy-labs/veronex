'use client'

/**
 * Provider Flow Panel — ArgoCD-style topology flow chart
 *
 * Node shapes per role:
 *   API (Veronex)   — rounded rectangle with left accent bar (gateway)
 *   Queue (Valkey)  — cylinder (storage / queue shape)
 *   Providers       — octagon (clipped-corner rectangle, compute node)
 *
 * 3-phase bidirectional flow:
 *   Enqueue  (API → Queue):      new job placed in Valkey queue
 *   Dispatch (Queue → Provider): job dequeued, sent to provider
 *   Response (Provider → API):   result returned, bypasses Queue
 *
 * Animation: CSS offset-path + @keyframes bee-fly (GPU-composited)
 * State:     useReducer (SPAWN / EXPIRE)
 * Scaling:   ResizeObserver → transform:scale; max-width 680 px cap
 */

import { useEffect, useRef, useReducer, useMemo } from 'react'
import { useTranslation } from '@/i18n'
import type { Provider } from '@/lib/types'
import type { FlowEvent } from '@/hooks/use-inference-stream'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useLabSettings } from '@/components/lab-settings-provider'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI, JOB_STATUS_COLORS } from '@/lib/constants'
import { getOllamaProviders, getGeminiProviders } from '@/lib/utils'

/* ─── viewport ──────────────────────────────────────────────── */
const VIEW_W = 540
const VIEW_H = 264
const MAX_BEES = 30
const BEE_DURATION_MS = 1400
const ENQUEUE_COLOR = 'var(--theme-status-warning)'

/* ─── Column 1: Veronex API ─────────────────────────────────── */
const API_CX = 72
const API_CY = 132
const API_W  = 108
const API_H  = 56

/* ─── Column 2: Queue (Valkey) — cylinder ───────────────────── */
const QUEUE_CX     = 244
const QUEUE_CY     = 132
const QUEUE_RX     = 44    // half-width (horizontal ellipse radius)
const QUEUE_RY     = 10    // cap depth  (vertical   ellipse radius)
const QUEUE_BODY_H = 44    // body rect height
const QUEUE_TOP_Y  = QUEUE_CY - QUEUE_BODY_H / 2  // 110
const QUEUE_BOT_Y  = QUEUE_CY + QUEUE_BODY_H / 2  // 154

/* ─── Column 3: Providers — octagon ─────────────────────────── */
const PROV_CX    = 460
const PROV_W     = 108
const PROV_H     = 52
const PROV_INSET = 10     // corner clip amount for octagon
const OLLAMA_CY  = 72
const GEMINI_CY  = 192

/* ─── connection endpoints ──────────────────────────────────── */
const API_RIGHT   = API_CX   + API_W  / 2  // 126
const QUEUE_LEFT  = QUEUE_CX - QUEUE_RX    // 200
const QUEUE_RIGHT = QUEUE_CX + QUEUE_RX    // 288
const PROV_LEFT   = PROV_CX  - PROV_W / 2  // 406

/* ─── paths: enqueue (API → Queue) ──────────────────────────── */
const PATH_API_QUEUE =
  `M ${API_RIGHT},${API_CY} C ${API_RIGHT + 24},${API_CY} ${QUEUE_LEFT - 24},${QUEUE_CY} ${QUEUE_LEFT},${QUEUE_CY}`

/* ─── paths: dispatch (Queue → Provider) ────────────────────── */
const PATH_QUEUE_OLLAMA =
  `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 30},${QUEUE_CY} ${PROV_LEFT - 30},${OLLAMA_CY} ${PROV_LEFT},${OLLAMA_CY}`
const PATH_QUEUE_GEMINI =
  `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 30},${QUEUE_CY} ${PROV_LEFT - 30},${GEMINI_CY} ${PROV_LEFT},${GEMINI_CY}`

/* ─── paths: response bypass arcs (Provider → API) ──────────── */
const PATH_OLLAMA_API =
  `M ${PROV_LEFT},${OLLAMA_CY} C ${PROV_LEFT - 60},${OLLAMA_CY - 54} ${API_RIGHT + 60},${OLLAMA_CY - 54} ${API_RIGHT},${API_CY}`
const PATH_GEMINI_API =
  `M ${PROV_LEFT},${GEMINI_CY} C ${PROV_LEFT - 60},${GEMINI_CY + 54} ${API_RIGHT + 60},${GEMINI_CY + 54} ${API_RIGHT},${API_CY}`

/* ─── helpers ────────────────────────────────────────────────── */
function statusColor(status: string): string {
  return JOB_STATUS_COLORS[status] ?? 'var(--theme-status-warning)'
}

function providerStroke(providers: Provider[]): string {
  if (providers.length === 0)                        return 'var(--theme-border)'
  if (providers.some(b => b.status === 'online'))    return 'var(--theme-status-success)'
  if (providers.some(b => b.status === 'degraded'))  return 'var(--theme-status-warning)'
  return 'var(--theme-status-error)'
}

function octPoints(cx: number, cy: number, w: number, h: number, inset: number): string {
  const x0 = cx - w / 2, x1 = cx + w / 2
  const y0 = cy - h / 2, y1 = cy + h / 2
  return [
    `${x0 + inset},${y0}`, `${x1 - inset},${y0}`,
    `${x1},${y0 + inset}`, `${x1},${y1 - inset}`,
    `${x1 - inset},${y1}`, `${x0 + inset},${y1}`,
    `${x0},${y1 - inset}`, `${x0},${y0 + inset}`,
  ].join(' ')
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

/* ─── Panel ──────────────────────────────────────────────────── */
interface Props {
  providers: Provider[]
  events: FlowEvent[]
  queueDepth?: number
}

export function ProviderFlowPanel({ providers, events, queueDepth = 0 }: Props) {
  const { t } = useTranslation()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const spawnedRef   = useRef(new Set<string>())
  const containerRef = useRef<HTMLDivElement>(null)
  const [scale,  setScale]  = useReducer((_: number, v: number) => v, 1)
  const [bees,   dispatch]  = useReducer(beeReducer, [])

  const localBs = useMemo(() => getOllamaProviders(providers), [providers])
  const apiBs   = useMemo(
    () => geminiEnabled ? getGeminiProviders(providers) : [],
    [providers, geminiEnabled],
  )

  // Responsive scaling
  useEffect(() => {
    if (!containerRef.current) return
    const obs = new ResizeObserver(([entry]) => {
      setScale(entry.contentRect.width / VIEW_W)
    })
    obs.observe(containerRef.current)
    return () => obs.disconnect()
  }, [])

  // Spawn bees for new events
  useEffect(() => {
    const newEvs = events.filter(e => !spawnedRef.current.has(e.id))
    if (newEvs.length === 0) return
    newEvs.forEach(e => spawnedRef.current.add(e.id))

    const newBees: Bee[] = []
    for (const e of newEvs) {
      const color = statusColor(e.status)

      if (e.phase === 'enqueue') {
        newBees.push({ id: `${e.id}-eq`, pathD: PATH_API_QUEUE, color: ENQUEUE_COLOR, phase: 'enqueue', delay: 0 })
      } else if (e.phase === 'dispatch') {
        const pathD = e.provider === PROVIDER_GEMINI ? PATH_QUEUE_GEMINI : PATH_QUEUE_OLLAMA
        newBees.push({ id: `${e.id}-qp`, pathD, color, phase: 'dispatch', delay: 0 })
      } else {
        const pathD = e.provider === PROVIDER_GEMINI ? PATH_GEMINI_API : PATH_OLLAMA_API
        newBees.push({ id: `${e.id}-pa`, pathD, color, phase: 'response', delay: 0 })
      }
    }

    if (newBees.length > 0) dispatch({ type: 'SPAWN', bees: newBees })
  }, [events])

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold">{t('overview.providerFlow')}</CardTitle>
      </CardHeader>
      <CardContent className="p-0 pb-2">
        {/* Max-width cap — prevents the SVG from filling ultra-wide screens */}
        <div className="mx-auto w-full" style={{ maxWidth: 680 }}>
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
              <defs>
                {/* Arrowhead markers */}
                <marker id="pfp-arrow" markerWidth="8" markerHeight="6"
                  refX="7" refY="3" orient="auto">
                  <polygon points="0 0, 8 3, 0 6"
                    style={{ fill: 'var(--theme-border)' }} />
                </marker>
                {/* API node left-accent clip */}
                <clipPath id="pfp-api-clip">
                  <rect
                    x={API_CX - API_W / 2} y={API_CY - API_H / 2}
                    width={API_W} height={API_H} rx={8}
                  />
                </clipPath>
              </defs>

              {/* ── Connection lines ─────────────────────────── */}

              {/* API → Queue (main dispatch path) */}
              <path d={PATH_API_QUEUE} fill="none" markerEnd="url(#pfp-arrow)"
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '6 4' }} />

              {/* Queue → Ollama */}
              <path d={PATH_QUEUE_OLLAMA} fill="none" markerEnd="url(#pfp-arrow)"
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '6 4' }} />

              {/* Queue → Gemini (lab-gated) */}
              {geminiEnabled && (
                <path d={PATH_QUEUE_GEMINI} fill="none" markerEnd="url(#pfp-arrow)"
                  style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5, strokeDasharray: '6 4' }} />
              )}

              {/* Response arcs — dimmed (bypass Queue) */}
              <path d={PATH_OLLAMA_API} fill="none"
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1, strokeDasharray: '3 7', opacity: 0.4 }} />
              {geminiEnabled && (
                <path d={PATH_GEMINI_API} fill="none"
                  style={{ stroke: 'var(--theme-border)', strokeWidth: 1, strokeDasharray: '3 7', opacity: 0.4 }} />
              )}

              {/* ── Node 1: Veronex API — rounded rect with left accent ── */}
              {/* Drop shadow */}
              <rect
                x={API_CX - API_W / 2 + 2} y={API_CY - API_H / 2 + 2}
                width={API_W} height={API_H} rx={8}
                style={{ fill: 'rgba(0,0,0,0.15)' }}
              />
              {/* Card body */}
              <rect
                x={API_CX - API_W / 2} y={API_CY - API_H / 2}
                width={API_W} height={API_H} rx={8}
                style={{ fill: 'var(--theme-bg-card)', stroke: 'var(--theme-primary)', strokeWidth: 1.5 }}
              />
              {/* Left accent bar (clipped to card shape) */}
              <rect
                x={API_CX - API_W / 2} y={API_CY - API_H / 2}
                width={5} height={API_H}
                clipPath="url(#pfp-api-clip)"
                style={{ fill: 'var(--theme-primary)' }}
              />
              {/* Labels */}
              <text x={API_CX + 2} y={API_CY - 5} textAnchor="middle"
                style={{ fill: 'var(--theme-text-primary)', fontSize: 11, fontWeight: 700 }}>
                Veronex API
              </text>
              <text x={API_CX + 2} y={API_CY + 10} textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 9 }}>
                {t('overview.flowHttpGateway')}
              </text>

              {/* ── Node 2: Queue (Valkey) — cylinder ────────────────── */}
              {/* Bottom ellipse cap — drawn first (behind body) */}
              <ellipse cx={QUEUE_CX} cy={QUEUE_BOT_Y} rx={QUEUE_RX} ry={QUEUE_RY}
                style={{ fill: 'var(--theme-bg-card)', stroke: 'var(--theme-border)', strokeWidth: 1.5 }} />
              {/* Cylinder body fill (no stroke — side lines drawn below) */}
              <rect
                x={QUEUE_LEFT} y={QUEUE_TOP_Y}
                width={QUEUE_RX * 2} height={QUEUE_BODY_H}
                style={{ fill: 'var(--theme-bg-card)', stroke: 'none' }}
              />
              {/* Left and right side lines */}
              <line x1={QUEUE_LEFT}  y1={QUEUE_TOP_Y} x2={QUEUE_LEFT}  y2={QUEUE_BOT_Y}
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5 }} />
              <line x1={QUEUE_RIGHT} y1={QUEUE_TOP_Y} x2={QUEUE_RIGHT} y2={QUEUE_BOT_Y}
                style={{ stroke: 'var(--theme-border)', strokeWidth: 1.5 }} />
              {/* Top ellipse cap — drawn last (in front, slightly elevated fill) */}
              <ellipse cx={QUEUE_CX} cy={QUEUE_TOP_Y} rx={QUEUE_RX} ry={QUEUE_RY}
                style={{ fill: 'var(--theme-bg-elevated)', stroke: 'var(--theme-border)', strokeWidth: 1.5 }} />
              {/* Labels (inside body) */}
              <text x={QUEUE_CX} y={QUEUE_CY - 4} textAnchor="middle"
                style={{ fill: 'var(--theme-text-primary)', fontSize: 11, fontWeight: 700 }}>
                Queue
              </text>
              <text x={QUEUE_CX} y={QUEUE_CY + 10} textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 9 }}>
                Valkey
              </text>
              {/* Queue depth badge — shown only when jobs are waiting */}
              {queueDepth > 0 && (
                <>
                  <rect
                    x={QUEUE_CX - 24} y={QUEUE_BOT_Y + QUEUE_RY + 3}
                    width={48} height={15} rx={7}
                    style={{ fill: 'color-mix(in srgb, var(--theme-status-warning) 12%, transparent)', stroke: 'var(--theme-status-warning)', strokeWidth: 1 }}
                  />
                  <text
                    x={QUEUE_CX} y={QUEUE_BOT_Y + QUEUE_RY + 13}
                    textAnchor="middle"
                    style={{ fill: 'var(--theme-status-warning)', fontSize: 9, fontWeight: 700 }}
                  >
                    {t('overview.queueWaiting', { count: queueDepth })}
                  </text>
                </>
              )}

              {/* ── Node 3a: Ollama — octagon ────────────────────────── */}
              {/* Drop shadow */}
              <polygon
                points={octPoints(PROV_CX + 2, OLLAMA_CY + 2, PROV_W, PROV_H, PROV_INSET)}
                style={{ fill: 'rgba(0,0,0,0.12)' }}
              />
              <polygon
                points={octPoints(PROV_CX, OLLAMA_CY, PROV_W, PROV_H, PROV_INSET)}
                style={{ fill: 'var(--theme-bg-card)', stroke: providerStroke(localBs), strokeWidth: 1.5 }}
              />
              <text x={PROV_CX} y={OLLAMA_CY - 4} textAnchor="middle"
                style={{ fill: 'var(--theme-text-primary)', fontSize: 11, fontWeight: 600 }}>
                Ollama
              </text>
              <text x={PROV_CX} y={OLLAMA_CY + 10} textAnchor="middle"
                style={{ fill: 'var(--theme-text-secondary)', fontSize: 8 }}>
                {localBs.length > 0
                  ? t('overview.flowOnlineCount', { online: localBs.filter(b => b.status === 'online').length, total: localBs.length })
                  : t('overview.flowNoProviders')}
              </text>

              {/* ── Node 3b: Gemini — octagon (lab-gated) ───────────── */}
              {geminiEnabled && (
                <>
                  {/* Drop shadow */}
                  <polygon
                    points={octPoints(PROV_CX + 2, GEMINI_CY + 2, PROV_W, PROV_H, PROV_INSET)}
                    style={{ fill: 'rgba(0,0,0,0.12)' }}
                  />
                  <polygon
                    points={octPoints(PROV_CX, GEMINI_CY, PROV_W, PROV_H, PROV_INSET)}
                    style={{ fill: 'var(--theme-bg-card)', stroke: providerStroke(apiBs), strokeWidth: 1.5 }}
                  />
                  <text x={PROV_CX} y={GEMINI_CY - 4} textAnchor="middle"
                    style={{ fill: 'var(--theme-text-primary)', fontSize: 11, fontWeight: 600 }}>
                    Gemini
                  </text>
                  <text x={PROV_CX} y={GEMINI_CY + 10} textAnchor="middle"
                    style={{ fill: 'var(--theme-text-secondary)', fontSize: 8 }}>
                    {apiBs.length > 0
                      ? t('overview.flowOnlineCount', { online: apiBs.filter(b => b.status === 'online').length, total: apiBs.length })
                      : t('overview.flowNoProviders')}
                  </text>
                </>
              )}
            </svg>

            {/* Bee overlay — positioned in SVG coordinate space then scaled */}
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
        </div>
      </CardContent>
    </Card>
  )
}
