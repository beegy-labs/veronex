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

import { memo, useEffect, useRef, useReducer, useMemo } from 'react'
import { useTranslation } from '@/i18n'
import type { Provider } from '@/lib/types'
import type { FlowEvent } from '@/hooks/use-inference-stream'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useLabSettings } from '@/components/lab-settings-provider'
import { PROVIDER_GEMINI, JOB_STATUS_COLORS } from '@/lib/constants'
import { getOllamaProviders, getGeminiProviders } from '@/lib/utils'
import { tokens } from '@/lib/design-tokens'

/* ─── viewport ──────────────────────────────────────────────── */
const VIEW_W = 540
const VIEW_H = 264
const MAX_BEES = 30
const ENQUEUE_COLOR = tokens.status.warning

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
// When both providers: Ollama top, Gemini bottom. When Ollama only: centered.
const OLLAMA_CY_DUAL   = 72
const GEMINI_CY_DUAL   = 192
const OLLAMA_CY_SINGLE = QUEUE_CY  // align with Queue center = straight line

/* ─── connection endpoints ──────────────────────────────────── */
const API_RIGHT   = API_CX   + API_W  / 2  // 126
const QUEUE_LEFT  = QUEUE_CX - QUEUE_RX    // 200
const QUEUE_RIGHT = QUEUE_CX + QUEUE_RX    // 288
const PROV_LEFT   = PROV_CX  - PROV_W / 2  // 406

/* ─── paths: enqueue (API → Queue) ──────────────────────────── */
const PATH_API_QUEUE =
  `M ${API_RIGHT},${API_CY} C ${API_RIGHT + 24},${API_CY} ${QUEUE_LEFT - 24},${QUEUE_CY} ${QUEUE_LEFT},${QUEUE_CY}`

/* ─── path builders (depend on runtime ollamaCy) ─────────────── */
function pathQueueOllama(ollamaCy: number) {
  return `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 30},${QUEUE_CY} ${PROV_LEFT - 30},${ollamaCy} ${PROV_LEFT},${ollamaCy}`
}
const PATH_QUEUE_GEMINI =
  `M ${QUEUE_RIGHT},${QUEUE_CY} C ${QUEUE_RIGHT + 30},${QUEUE_CY} ${PROV_LEFT - 30},${GEMINI_CY_DUAL} ${PROV_LEFT},${GEMINI_CY_DUAL}`

function pathOllamaApi(ollamaCy: number) {
  return `M ${PROV_LEFT},${ollamaCy} C ${PROV_LEFT - 60},${ollamaCy - 54} ${API_RIGHT + 60},${ollamaCy - 54} ${API_RIGHT},${API_CY}`
}
const PATH_GEMINI_API =
  `M ${PROV_LEFT},${GEMINI_CY_DUAL} C ${PROV_LEFT - 60},${GEMINI_CY_DUAL + 54} ${API_RIGHT + 60},${GEMINI_CY_DUAL + 54} ${API_RIGHT},${API_CY}`

/* ─── helpers ────────────────────────────────────────────────── */
function statusColor(status: string): string {
  return JOB_STATUS_COLORS[status] ?? tokens.status.warning
}

function providerStroke(providers: Provider[]): string {
  // Empty = still loading (query cache warming) — show neutral, not error
  if (providers.length === 0)                        return tokens.border.subtle
  if (providers.some(b => b.status === 'online'))    return tokens.status.success
  if (providers.some(b => b.status === 'degraded'))  return tokens.status.warning
  return tokens.status.error
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

/* ─── bee sizing — scales particle with volume ───────────────── */
// Maps a count (0–20+) to a pixel diameter (6–18px).
function beeSize(count: number): number {
  return Math.min(6 + Math.floor(count * 0.6), 18)
}

/* ─── bee reducer ────────────────────────────────────────────── */
type Bee = {
  id: string
  pathD: string
  color: string
  phase: 'enqueue' | 'dispatch' | 'response'
  size: number
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
  pendingJobs?: number
  runningJobs?: number
  /** req/s (10-second smoothed) */
  recentRequests?: number
  /** req/m (60-second window) */
  reqPerMin?: number
}

export const ProviderFlowPanel = memo(function ProviderFlowPanel({ providers, events, pendingJobs = 0, runningJobs = 0, recentRequests = 0, reqPerMin = 0 }: Props) {
  const { t } = useTranslation()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  const spawnedRef   = useRef(new Set<string>())
  // Cap spawnedRef to prevent unbounded growth on long-lived sessions
  if (spawnedRef.current.size > 500) {
    const iter = spawnedRef.current.values()
    for (let i = 0; i < 250; i++) {
      const { value, done } = iter.next()
      if (done) break
      spawnedRef.current.delete(value)
    }
  }
  const containerRef = useRef<HTMLDivElement>(null)
  const [scale,  setScale]  = useReducer((_: number, v: number) => v, 1)
  const [bees,   dispatch]  = useReducer(beeReducer, [])

  const localBs = useMemo(() => getOllamaProviders(providers), [providers])
  const apiBs   = useMemo(
    () => geminiEnabled ? getGeminiProviders(providers) : [],
    [providers, geminiEnabled],
  )
  const localOnline = useMemo(() => localBs.filter(b => b.status === 'online').length, [localBs])
  const apiOnline   = useMemo(() => apiBs.filter(b => b.status === 'online').length, [apiBs])

  // Ollama Y position: centered when Gemini disabled, top when both active
  const ollamaCy = geminiEnabled ? OLLAMA_CY_DUAL : OLLAMA_CY_SINGLE
  const PATH_QUEUE_OLLAMA = useMemo(() => pathQueueOllama(ollamaCy), [ollamaCy])
  const PATH_OLLAMA_API   = useMemo(() => pathOllamaApi(ollamaCy), [ollamaCy])

  // Responsive scaling
  useEffect(() => {
    if (!containerRef.current) return
    const obs = new ResizeObserver(([entry]) => {
      setScale(entry.contentRect.width / VIEW_W)
    })
    obs.observe(containerRef.current)
    return () => obs.disconnect()
  }, [])

  // Bee sizes based on current volume
  const enqueueSize  = beeSize(recentRequests * 10)  // incoming rate
  const dispatchSize = beeSize(runningJobs)
  const responseSize = beeSize(runningJobs)

  // Spawn bees for new events (skip stale replayed events older than 2s)
  useEffect(() => {
    const now = Date.now()
    const newEvs = events.filter(e => !spawnedRef.current.has(e.id) && now - e.ts < 2000)
    if (newEvs.length === 0) return
    newEvs.forEach(e => spawnedRef.current.add(e.id))

    const newBees: Bee[] = []
    for (const e of newEvs) {
      const color = statusColor(e.status)

      if (e.phase === 'enqueue') {
        newBees.push({ id: `${e.id}-eq`, pathD: PATH_API_QUEUE, color: ENQUEUE_COLOR, phase: 'enqueue', size: enqueueSize, delay: 0 })
      } else if (e.phase === 'dispatch') {
        const pathD = e.provider === PROVIDER_GEMINI ? PATH_QUEUE_GEMINI : PATH_QUEUE_OLLAMA
        newBees.push({ id: `${e.id}-qp`, pathD, color, phase: 'dispatch', size: dispatchSize, delay: 0 })
      } else {
        const pathD = e.provider === PROVIDER_GEMINI ? PATH_GEMINI_API : PATH_OLLAMA_API
        newBees.push({ id: `${e.id}-pa`, pathD, color, phase: 'response', size: responseSize, delay: 0 })
      }
    }

    if (newBees.length > 0) dispatch({ type: 'SPAWN', bees: newBees })
  }, [events, enqueueSize, dispatchSize, responseSize, PATH_QUEUE_OLLAMA, PATH_OLLAMA_API])

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between gap-4">
          <CardTitle className="text-sm font-semibold">{t('overview.providerFlow')}</CardTitle>
          {(pendingJobs > 0 || runningJobs > 0 || recentRequests > 0) && (
            <div className="flex items-center gap-3 text-xs text-muted-foreground shrink-0">
              {pendingJobs > 0 && (
                <span style={{ color: tokens.status.warning }}>
                  {t('overview.pendingJobsCount', { count: pendingJobs })}
                </span>
              )}
              {runningJobs > 0 && (
                <span style={{ color: tokens.status.info }}>
                  {t('overview.runningJobsCount', { count: runningJobs })}
                </span>
              )}
              <span>
                {t('overview.reqLast30s', { count: recentRequests })}
              </span>
            </div>
          )}
        </div>
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
              aria-label={t('overview.providerFlow')}
            >
              <defs>
                {/* Arrowhead markers */}
                <marker id="pfp-arrow" markerWidth="8" markerHeight="6"
                  refX="7" refY="3" orient="auto">
                  <polygon points="0 0, 8 3, 0 6"
                    style={{ fill: tokens.border.base }} />
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
                style={{ stroke: tokens.border.base, strokeWidth: 1.5, strokeDasharray: '6 4' }} />

              {/* Queue → Ollama */}
              <path d={PATH_QUEUE_OLLAMA} fill="none" markerEnd="url(#pfp-arrow)"
                style={{ stroke: tokens.border.base, strokeWidth: 1.5, strokeDasharray: '6 4' }} />

              {/* Queue → Gemini (lab-gated) */}
              {geminiEnabled && (
                <path d={PATH_QUEUE_GEMINI} fill="none" markerEnd="url(#pfp-arrow)"
                  style={{ stroke: tokens.border.base, strokeWidth: 1.5, strokeDasharray: '6 4' }} />
              )}

              {/* Response arcs — dimmed (bypass Queue) */}
              <path d={PATH_OLLAMA_API} fill="none"
                style={{ stroke: tokens.border.base, strokeWidth: 1, strokeDasharray: '3 7', opacity: 0.4 }} />
              {geminiEnabled && (
                <path d={PATH_GEMINI_API} fill="none"
                  style={{ stroke: tokens.border.base, strokeWidth: 1, strokeDasharray: '3 7', opacity: 0.4 }} />
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
                style={{ fill: tokens.bg.card, stroke: tokens.brand.primary, strokeWidth: 1.5 }}
              />
              {/* Left accent bar (clipped to card shape) */}
              <rect
                x={API_CX - API_W / 2} y={API_CY - API_H / 2}
                width={5} height={API_H}
                clipPath="url(#pfp-api-clip)"
                style={{ fill: tokens.brand.primary }}
              />
              {/* Labels */}
              <text x={API_CX + 2} y={API_CY - 5} textAnchor="middle"
                style={{ fill: tokens.text.primary, fontSize: 11, fontWeight: 700 }}>
                {t('overview.flowApiNode')}
              </text>
              <text x={API_CX + 2} y={API_CY + 10} textAnchor="middle"
                style={{ fill: tokens.text.secondary, fontSize: 9 }}>
                {t('overview.flowHttpGateway')}
              </text>
              {/* API: req/s + req/m badge — always visible */}
              <rect x={API_CX - API_W / 2 + 4} y={API_CY + API_H / 2 + 3} width={API_W - 8} height={22} rx={7}
                style={{
                  fill: recentRequests > 0
                    ? `color-mix(in srgb, ${tokens.brand.primary} 12%, transparent)`
                    : `color-mix(in srgb, ${tokens.text.secondary} 8%, transparent)`,
                  stroke: recentRequests > 0 ? tokens.brand.primary : tokens.border.base,
                  strokeWidth: 1,
                }} />
              <text x={API_CX} y={API_CY + API_H / 2 + 13} textAnchor="middle"
                style={{ fill: recentRequests > 0 ? tokens.brand.primary : tokens.text.secondary, fontSize: 8, fontWeight: 700 }}>
                {t('overview.flowReqPerSec', { value: typeof recentRequests === 'number' ? recentRequests.toFixed(1) : recentRequests })}
              </text>
              <text x={API_CX} y={API_CY + API_H / 2 + 22} textAnchor="middle"
                style={{ fill: tokens.text.secondary, fontSize: 7 }}>
                {t('overview.flowReqPerMin', { value: reqPerMin })}
              </text>
              {/* ── Node 2: Queue (Valkey) — cylinder ────────────────── */}
              {/* Bottom ellipse cap — drawn first (behind body) */}
              <ellipse cx={QUEUE_CX} cy={QUEUE_BOT_Y} rx={QUEUE_RX} ry={QUEUE_RY}
                style={{ fill: tokens.bg.card, stroke: tokens.border.base, strokeWidth: 1.5 }} />
              {/* Cylinder body fill (no stroke — side lines drawn below) */}
              <rect
                x={QUEUE_LEFT} y={QUEUE_TOP_Y}
                width={QUEUE_RX * 2} height={QUEUE_BODY_H}
                style={{ fill: tokens.bg.card, stroke: 'none' }}
              />
              {/* Left and right side lines */}
              <line x1={QUEUE_LEFT}  y1={QUEUE_TOP_Y} x2={QUEUE_LEFT}  y2={QUEUE_BOT_Y}
                style={{ stroke: tokens.border.base, strokeWidth: 1.5 }} />
              <line x1={QUEUE_RIGHT} y1={QUEUE_TOP_Y} x2={QUEUE_RIGHT} y2={QUEUE_BOT_Y}
                style={{ stroke: tokens.border.base, strokeWidth: 1.5 }} />
              {/* Top ellipse cap — drawn last (in front, slightly elevated fill) */}
              <ellipse cx={QUEUE_CX} cy={QUEUE_TOP_Y} rx={QUEUE_RX} ry={QUEUE_RY}
                style={{ fill: tokens.bg.elevated, stroke: tokens.border.base, strokeWidth: 1.5 }} />
              {/* Labels (inside body) */}
              <text x={QUEUE_CX} y={QUEUE_CY - 4} textAnchor="middle"
                style={{ fill: tokens.text.primary, fontSize: 11, fontWeight: 700 }}>
                {t('overview.flowQueueNode')}
              </text>
              <text x={QUEUE_CX} y={QUEUE_CY + 10} textAnchor="middle"
                style={{ fill: tokens.text.secondary, fontSize: 9 }}>
                {t('overview.flowValkeyNode')}
              </text>
              {/* Queue: pending count badge — always visible */}
              <rect x={QUEUE_CX - 24} y={QUEUE_BOT_Y + QUEUE_RY + 3} width={48} height={14} rx={7}
                style={{
                  fill: pendingJobs > 0
                    ? `color-mix(in srgb, ${tokens.status.warning} 15%, transparent)`
                    : `color-mix(in srgb, ${tokens.text.secondary} 8%, transparent)`,
                  stroke: pendingJobs > 0 ? tokens.status.warning : tokens.border.base,
                  strokeWidth: 1,
                }} />
              <text x={QUEUE_CX} y={QUEUE_BOT_Y + QUEUE_RY + 13} textAnchor="middle"
                style={{ fill: pendingJobs > 0 ? tokens.status.warning : tokens.text.secondary, fontSize: 8, fontWeight: 700 }}>
                {t('overview.pendingJobsCount', { count: pendingJobs })}
              </text>
              {/* ── Node 3a: Ollama — octagon ────────────────────────── */}
              {/* Drop shadow */}
              <polygon
                points={octPoints(PROV_CX + 2, ollamaCy + 2, PROV_W, PROV_H, PROV_INSET)}
                style={{ fill: 'rgba(0,0,0,0.12)' }}
              />
              <polygon
                points={octPoints(PROV_CX, ollamaCy, PROV_W, PROV_H, PROV_INSET)}
                style={{ fill: tokens.bg.card, stroke: providerStroke(localBs), strokeWidth: 1.5 }}
              />
              <text x={PROV_CX} y={ollamaCy - 4} textAnchor="middle"
                style={{ fill: tokens.text.primary, fontSize: 11, fontWeight: 600 }}>
                {t('nav.ollama')}
              </text>
              <text x={PROV_CX} y={ollamaCy + 10} textAnchor="middle"
                style={{ fill: tokens.text.secondary, fontSize: 8 }}>
                {localBs.length > 0
                  ? t('overview.flowOnlineCount', { online: localOnline, total: localBs.length })
                  : t('overview.flowNoProviders')}
              </text>
              {/* Ollama: running count badge — always visible */}
              <rect x={PROV_CX - 24} y={ollamaCy + PROV_H / 2 + 3} width={48} height={14} rx={7}
                style={{
                  fill: runningJobs > 0
                    ? `color-mix(in srgb, ${tokens.status.info} 15%, transparent)`
                    : `color-mix(in srgb, ${tokens.text.secondary} 8%, transparent)`,
                  stroke: runningJobs > 0 ? tokens.status.info : tokens.border.base,
                  strokeWidth: 1,
                }} />
              <text x={PROV_CX} y={ollamaCy + PROV_H / 2 + 13} textAnchor="middle"
                style={{ fill: runningJobs > 0 ? tokens.status.info : tokens.text.secondary, fontSize: 8, fontWeight: 700 }}>
                {t('overview.runningJobsCount', { count: runningJobs })}
              </text>
              {/* ── Node 3b: Gemini — octagon (lab-gated) ───────────── */}
              {geminiEnabled && (
                <>
                  {/* Drop shadow */}
                  <polygon
                    points={octPoints(PROV_CX + 2, GEMINI_CY_DUAL + 2, PROV_W, PROV_H, PROV_INSET)}
                    style={{ fill: 'rgba(0,0,0,0.12)' }}
                  />
                  <polygon
                    points={octPoints(PROV_CX, GEMINI_CY_DUAL, PROV_W, PROV_H, PROV_INSET)}
                    style={{ fill: tokens.bg.card, stroke: providerStroke(apiBs), strokeWidth: 1.5 }}
                  />
                  <text x={PROV_CX} y={GEMINI_CY_DUAL - 4} textAnchor="middle"
                    style={{ fill: tokens.text.primary, fontSize: 11, fontWeight: 600 }}>
                    {t('nav.gemini')}
                  </text>
                  <text x={PROV_CX} y={GEMINI_CY_DUAL + 10} textAnchor="middle"
                    style={{ fill: tokens.text.secondary, fontSize: 8 }}>
                    {apiBs.length > 0
                      ? t('overview.flowOnlineCount', { online: apiOnline, total: apiBs.length })
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
                    '--bee-size': `${bee.size}px`,
                    offsetPath:      `path("${bee.pathD}")`,
                    backgroundColor: bee.phase === 'response' ? `${bee.color}cc` : bee.color,
                    boxShadow:       `0 0 ${Math.round(bee.size * 0.6)}px ${Math.round(bee.size * 0.2)}px ${bee.color}${bee.phase === 'response' ? '28' : '44'}`,
                    animationDelay:  bee.delay > 0 ? `${bee.delay}ms` : undefined,
                  } as React.CSSProperties}
                  onAnimationEnd={() => dispatch({ type: 'EXPIRE', id: bee.id })}
                />
              ))}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
})
