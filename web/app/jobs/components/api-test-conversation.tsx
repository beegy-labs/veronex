'use client'

import { memo, useEffect, useRef, useCallback } from 'react'
import { useImageDrop } from '@/hooks/use-image-drop'
import { ImageAttachButton } from './image-attach-button'
import { Trash2, Square, Send, X, Loader2, Plus, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { CopyButton } from '@/components/copy-button'
import { renderWithMermaid } from '@/components/mermaid-block'
import { TurnInternals } from '@/components/turn-internals'
import { MAX_CONV_SESSIONS } from './api-test-types'
import type { ConversationMessage, ConversationSession, StreamStatus } from './api-test-types'

interface ApiTestConversationProps {
  sessions: ConversationSession[]
  activeSessionId: number | null
  messages: ConversationMessage[]
  streamingText: string
  status: StreamStatus
  errorMsg: string
  mcpToolCall?: string
  // Input area props
  prompt: string
  images: string[]
  maxImages: number
  isCompressing: boolean
  isGeminiProvider: boolean
  canRun: boolean
  useMcp: boolean
  onUseMcpChange: (v: boolean) => void
  onNewSession: () => void
  onCloseSession: (id: number) => void
  onSelectSession: (id: number) => void
  onPromptChange: (v: string) => void
  onImageAdd: (files: FileList) => void
  onImageRemove: (index: number) => void
  onRun: () => void
  onClear: () => void
  onStop: () => void
}

export const ApiTestConversation = memo(function ApiTestConversation({
  sessions, activeSessionId,
  messages, streamingText, status, errorMsg, mcpToolCall,
  prompt, images, maxImages, isCompressing, isGeminiProvider, canRun,
  useMcp, onUseMcpChange,
  onNewSession, onCloseSession, onSelectSession,
  onPromptChange, onImageAdd, onImageRemove, onRun,
  onClear, onStop,
}: ApiTestConversationProps) {
  const { t } = useTranslation()
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [messages.length, streamingText])

  const canAddMore = images.length < maxImages && !isGeminiProvider && maxImages > 0
  const { isDragging, handleDragOver, handleDragLeave, handleDrop } = useImageDrop(canAddMore, onImageAdd)

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      if (canRun) onRun()
    }
  }, [canRun, onRun])

  const turnCount = messages.filter((m) => m.role === 'user').length
  const hasContent = messages.length > 0 || status !== 'idle'
  const isEmpty = sessions.length === 0

  // Session tab label: first user message truncated, or fallback
  function sessionLabel(s: ConversationSession) {
    const first = s.messages.find((m) => m.role === 'user')
    if (!first) return `#${s.id}`
    const text = first.content.trim()
    return text.length > 20 ? text.slice(0, 20) + '…' : text
  }

  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null

  return (
    <div
      className={`vds-border-1 vds-border-subtle vds-rounded-md vds-overflow-hidden vds-flex vds-flex-col vds-h-[520px]${isDragging ? ' vds-ring-2 vds-ring-focus vds-ring-offset-2' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Session tab strip */}
      <div className="vds-flex vds-items-center vds-gap-0 vds-border-b-1 vds-border-subtle vds-bg-muted/20 vds-overflow-x-auto">
        {sessions.map((s) => (
          <div
            key={s.id}
            role="button"
            tabIndex={0}
            className={`vds-flex vds-items-center vds-gap-1.5 vds-px-3 vds-py-1.5 vds-text-xs vds-font-500 vds-cursor-pointer vds-select-none vds-transition-colors vds-flex-shrink-0 vds-border-r-1 vds-border-subtle ${
 s.id === activeSessionId
 ? 'vds-bg-card vds-text-primary'
 : 'vds-text-dim vds-hover:text-primary vds-hover:bg-hover/50'
 }`}
            onClick={() => onSelectSession(s.id)}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectSession(s.id) } }}
          >
            {s.status === 'streaming' && (
              <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-info-bg-fg vds-animate-pulse vds-flex-shrink-0" />
            )}
            {s.status === 'error' && (
              <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-flex-shrink-0" />
            )}
            <span className="vds-max-w-[120px] vds-truncate">{sessionLabel(s)}</span>
            <button
              type="button"
              aria-label={t('common.close')}
              className="vds-ml-0.5 vds-rounded vds-hover:bg-destructive/20 vds-hover:text-destructive vds-p-0.5 vds--mr-1"
              onClick={(e) => { e.stopPropagation(); onCloseSession(s.id) }}
            >
              <X className="vds-h-3 vds-w-3" />
            </button>
          </div>
        ))}
        {sessions.length < MAX_CONV_SESSIONS && (
          <button
            type="button"
            onClick={onNewSession}
            className="vds-flex vds-items-center vds-gap-1 vds-px-2 vds-py-1.5 vds-text-xs vds-text-dim vds-hover:text-primary vds-transition-colors vds-flex-shrink-0"
            aria-label={t('test.newSession')}
          >
            <Plus className="vds-h-3.5 vds-w-3.5" />
          </button>
        )}
        {/* Spacer + turn count + stop/clear on active session */}
        {activeSessionId !== null && hasContent && (
          <div className="vds-flex vds-items-center vds-gap-1 vds-ml-auto vds-px-2 vds-flex-shrink-0">
            {activeSession?.conversationId && (
              <span
                className="vds-font-mono vds-text-xs vds-text-dim/60 vds-select-all vds-cursor-text"
                title="conversation_id"
              >
                {activeSession.conversationId}
              </span>
            )}
            <span className="vds-text-xs vds-text-dim">{turnCount} {t('test.turns')}</span>
            {status === 'streaming' && (
              <Button type="button" variant="ghost" size="sm" onClick={onStop}
                className="vds-h-6 vds-text-xs vds-text-dim vds-hover:text-primary">
                <Square className="vds-h-3 vds-w-3 vds-mr-1" fill="currentColor" />
                {t('test.stop')}
              </Button>
            )}
            <Button type="button" variant="ghost" size="sm" onClick={onClear}
              disabled={status === 'streaming'}
              className="vds-h-6 vds-text-xs vds-text-dim vds-hover:text-destructive">
              <Trash2 className="vds-h-3 vds-w-3 vds-mr-1" />
              {t('test.clearConversation')}
            </Button>
          </div>
        )}
      </div>

      {/* Empty state */}
      {isEmpty && (
        <div className="vds-flex-1 vds-flex vds-flex-col vds-items-center vds-justify-center vds-py-8 vds-text-dim vds-text-sm vds-gap-2">
          <p>{t('test.noSessions')}</p>
          <Button type="button" variant="outline" size="sm" onClick={onNewSession}>
            <Plus className="vds-h-3.5 vds-w-3.5 vds-mr-1" />
            {t('test.newSession')}
          </Button>
        </div>
      )}

      {/* Message thread */}
      {!isEmpty && (
        <div ref={scrollRef} className="vds-flex-1 vds-overflow-y-auto vds-p-3 vds-space-y-3 vds-min-h-0">
          {messages.map((msg, i) =>
            msg.role === 'system' ? (
              <div key={`msg-${i}-system`} className="vds-flex vds-items-center vds-gap-2 vds-py-1">
                <div className="vds-flex-1 vds-h-px vds-bg-border-subtle/60" />
                <span className="vds-text-2xs vds-text-dim/70 vds-flex-shrink-0">{msg.content}</span>
                <div className="vds-flex-1 vds-h-px vds-bg-border-subtle/60" />
              </div>
            ) : msg.role === 'user' ? (
              <div key={`msg-${i}-user`} className="vds-flex vds-justify-end">
                <div className="vds-max-w-[80%] vds-rounded-2xl vds-rounded-tr-sm vds-px-3 vds-py-2 vds-bg-primary vds-text-primary-fg vds-text-sm">
                  {msg.images && msg.images.length > 0 && (
                    <div className="vds-flex vds-gap-1 vds-mb-2 vds-flex-wrap">
                      {msg.images.map((b64, j) => (
                        // eslint-disable-next-line @next/next/no-img-element
                        <img key={b64.slice(0, 16)} src={`data:image/jpeg;base64,${b64}`} alt="" className="vds-h-12 vds-w-12 vds-rounded vds-object-cover" />
                      ))}
                    </div>
                  )}
                  <span className="vds-whitespace-pre-wrap vds-break-words">{msg.content}</span>
                </div>
              </div>
            ) : (
              <div key={`msg-${i}-assistant`} className="vds-flex vds-justify-start">
                <div className="vds-max-w-[80%] vds-relative vds-group">
                  {msg.model && (
                    <div className="vds-mb-1 vds-px-1">
                      <span className="vds-text-xs vds-text-dim/60 vds-font-mono">{msg.model}</span>
                    </div>
                  )}
                  <div className="vds-rounded-2xl vds-rounded-tl-sm vds-px-3 vds-py-2 vds-bg-muted vds-text-primary vds-text-sm vds-font-mono vds-leading-relaxed">
                    {msg.content
                      ? renderWithMermaid(msg.content, false)
                      : msg.hasMcpTools
                        ? <span className="vds-text-xs vds-text-dim/70 vds-italic">{t('test.toolOnlyTurn')}</span>
                        : renderWithMermaid(msg.content, false)}
                  </div>
                  {/* SDD §3 Tier B — show the model's emitted tool_calls
 vds-inline (S3 TurnRecord-sourced, populated after the SSE
 stream completes). Renders even when content is empty,
 so tool-only turns are no longer "결과 없음". */}
 {msg.toolCalls && msg.toolCalls.length > 0 && (
 <div className="vds-mt-1.5 vds-space-y-1">
 {msg.toolCalls.map((tc, j) => (
 <div key={`tc-${j}-${tc.name}`} className="vds-rounded vds-border-1 vds-border-subtle/60 vds-bg-page vds-px-2 vds-py-1.5">
 <div className="vds-flex vds-items-center vds-gap-1.5">
 <Wrench className="vds-h-3 vds-w-3 vds-text-info vds-flex-shrink-0" />
 <code className="vds-text-2xs vds-font-mono vds-font-600 vds-text-info vds-break-all">{tc.name}</code>
 </div>
 {tc.arguments != null && (
 <pre className="vds-text-[10px] vds-font-mono vds-text-primary/60 vds-mt-1 vds-whitespace-pre-wrap vds-break-words vds-max-h-24 vds-overflow-y-auto">
 {typeof tc.arguments ==='string'? tc.arguments : JSON.stringify(tc.arguments, null, 2)}
 </pre>
 )}
 </div>
 ))}
 </div>
 )}
 {/* PG audit (latency / cache_hit / outcome). Lazy — user
 clicks to expand. Key off jobId being a real persisted
 job_id (resolved from S3 turn list, not chunk.id). */}
 {msg.jobId && msg.hasMcpTools && activeSession?.conversationId && (
 <div className="vds-mt-1 vds-px-1">
 <TurnInternals
 convId={activeSession.conversationId}
 jobId={msg.jobId}
 />
 </div>
 )}
 <div className="vds-absolute vds-top-1 vds-right-1 vds-opacity-0 vds-group-hover:opacity-100 vds-transition-opacity">
 <CopyButton text={msg.content} />
 </div>
 </div>
 </div>
 )
 )}

 {status ==='streaming'&& (
 <div className="vds-flex vds-justify-start">
 <div className="vds-max-w-[80%] vds-rounded-2xl vds-rounded-tl-sm vds-px-3 vds-py-2 vds-bg-muted vds-text-primary vds-text-sm vds-font-mono vds-leading-relaxed">
 {streamingText
 ? renderWithMermaid(streamingText, true)
 : (
 <span className="vds-inline-flex vds-flex-col vds-gap-1">
 <span className="vds-inline-flex vds-gap-1 vds-items-center vds-text-dim">
 <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-neutral vds-animate-bounce" style={{ animationDelay:'0ms'}} />
 <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-neutral vds-animate-bounce" style={{ animationDelay:'150ms'}} />
 <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-neutral vds-animate-bounce" style={{ animationDelay:'300ms'}} />
 </span>
 {mcpToolCall && (
 <span className="vds-inline-flex vds-items-center vds-gap-1 vds-text-2xs vds-text-dim/70 vds-font-mono">
 <Wrench className="vds-h-3 vds-w-3 vds-flex-shrink-0" />
 {mcpToolCall}
 </span>
 )}
 </span>
 )
 }
 </div>
 </div>
 )}

 {status ==='error'&& (
 <div className="vds-rounded-md vds-border-1 vds-border-error/30 vds-bg-error/5 vds-px-3 vds-py-2 vds-text-sm vds-text-error">
 {errorMsg}
 </div>
 )}

 </div>
 )}

 {/* Input area — vds-hidden when no sessions */}
 {!isEmpty && (
 <div className="vds-px-4 vds-pt-3 vds-pb-3 vds-border-t-1 vds-border-subtle vds-flex-shrink-0">
 {/* Image thumbnails above input */}
 {images.length > 0 && (
 <div className="vds-flex vds-flex-wrap vds-gap-2 vds-mb-2">
 {images.map((b64, i) => (
 <div key={b64.slice(0, 16)} className="vds-relative vds-group">
 {/* eslint-disable-next-line @next/next/no-img-element */}
 <img
 src={`data:image/jpeg;base64,${b64}`}
 alt={`image-${i + 1}`}
 className="vds-h-12 vds-w-12 vds-rounded-md vds-object-cover vds-border-1 vds-border-default"
 />
 <button
 type="button"
 onClick={() => onImageRemove(i)}
 aria-label={t('test.imageRemove')}
 className="vds-absolute vds--top-1.5 vds--right-1.5 vds-hidden vds-group-hover:flex vds-h-4 vds-w-4 vds-items-center vds-justify-center vds-rounded-full vds-bg-destructive vds-text-destructive-fg"
 >
 <X className="vds-h-2.5 vds-w-2.5" />
 </button>
 </div>
 ))}
 {isCompressing && (
 <div className="vds-flex vds-h-12 vds-w-12 vds-items-center vds-justify-center vds-rounded-md vds-border-1 vds-border-dashed vds-border-default">
 <Loader2 className="vds-h-5 vds-w-5 vds-animate-spin vds-text-dim" aria-label={t('test.imageCompressing')} />
                </div>
              )}
            </div>
          )}

          <textarea
            value={prompt}
            onChange={(e) => onPromptChange(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={3}
            placeholder={t('test.promptPlaceholder')}
            disabled={status === 'streaming'}
 className="vds-w-full vds-rounded-md vds-border-0 vds-bg-transparent vds-px-0 vds-py-1 vds-text-sm vds-placeholder:text-dim vds-focus-visible:outline-none vds-disabled:cursor-not-allowed vds-disabled:opacity-50 vds-resize-none"
 />
 {/* Gmail-style bottom toolbar */}
 <div className="vds-flex vds-items-center vds-gap-2 vds-pt-2 vds-border-t-1 vds-border-subtle/50">
 <Button
 type="button"
 onClick={onRun}
 disabled={!canRun}
 className="vds-rounded-full vds-px-5 vds-h-8 vds-text-sm vds-font-medium"
 aria-label={t('test.run')}
 >
 <Send className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
 {t('test.run')}
            </Button>
            {!isGeminiProvider && (
              <ImageAttachButton
                canAddMore={canAddMore}
                isCompressing={isCompressing}
                onImageAdd={onImageAdd}
              />
            )}
            {!isGeminiProvider && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => onUseMcpChange(!useMcp)}
                title={useMcp ? t('test.mcpDisable') : t('test.mcpEnable')}
 className={`vds-h-8 vds-px-2 vds-text-xs vds-gap-1 ${useMcp ? 'vds-text-dim vds-hover:text-primary' : 'vds-text-dim/40 vds-hover:text-dim'}`}
 >
 <Wrench className="vds-h-3.5 vds-w-3.5" />
 {t('test.mcp')}
              </Button>
            )}
            <span className="vds-ml-auto vds-text-xs vds-text-dim/50">⌘↵</span>
          </div>
        </div>
      )}
    </div>
  )
})
