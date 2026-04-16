'use client'

import { memo, useEffect, useRef, useCallback, useState } from 'react'
import { Trash2, Square, Send, ImagePlus, X, Loader2, Plus, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { CopyButton } from '@/components/copy-button'
import { renderWithMermaid } from '@/components/mermaid-block'
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
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [messages.length, streamingText])

  const canAddMore = images.length < maxImages && !isGeminiProvider && maxImages > 0

  const handleFileChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files.length > 0) onImageAdd(e.target.files)
    e.target.value = ''
  }, [onImageAdd])

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      if (canRun) onRun()
    }
  }, [canRun, onRun])

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    if (canAddMore) setIsDragging(true)
  }, [canAddMore])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }, [])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
    if (!canAddMore) return
    const files = e.dataTransfer.files
    if (files.length > 0) {
      const imageFiles = Array.from(files).filter((f) => f.type.startsWith('image/'))
      if (imageFiles.length > 0) {
        const dt = new DataTransfer()
        imageFiles.forEach((f) => dt.items.add(f))
        onImageAdd(dt.files)
      }
    }
  }, [canAddMore, onImageAdd])

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
      className={`border border-border rounded-md overflow-hidden flex flex-col h-[520px]${isDragging ? ' ring-2 ring-ring ring-offset-2' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Session tab strip */}
      <div className="flex items-center gap-0 border-b border-border bg-muted/20 overflow-x-auto">
        {sessions.map((s) => (
          <div
            key={s.id}
            role="button"
            tabIndex={0}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium cursor-pointer select-none transition-colors shrink-0 border-r border-border ${
              s.id === activeSessionId
                ? 'bg-card text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted/50'
            }`}
            onClick={() => onSelectSession(s.id)}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectSession(s.id) } }}
          >
            {s.status === 'streaming' && (
              <span className="h-1.5 w-1.5 rounded-full bg-status-info-fg animate-pulse shrink-0" />
            )}
            {s.status === 'error' && (
              <span className="h-1.5 w-1.5 rounded-full bg-status-error shrink-0" />
            )}
            <span className="max-w-[120px] truncate">{sessionLabel(s)}</span>
            <button
              type="button"
              aria-label={t('common.close')}
              className="ml-0.5 rounded hover:bg-destructive/20 hover:text-destructive p-0.5 -mr-1"
              onClick={(e) => { e.stopPropagation(); onCloseSession(s.id) }}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ))}
        {sessions.length < MAX_CONV_SESSIONS && (
          <button
            type="button"
            onClick={onNewSession}
            className="flex items-center gap-1 px-2 py-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors shrink-0"
            aria-label={t('test.newSession')}
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
        )}
        {/* Spacer + turn count + stop/clear on active session */}
        {activeSessionId !== null && hasContent && (
          <div className="flex items-center gap-1 ml-auto px-2 shrink-0">
            {activeSession?.conversationId && (
              <span
                className="font-mono text-xs text-muted-foreground/60 select-all cursor-text"
                title="conversation_id"
              >
                {activeSession.conversationId}
              </span>
            )}
            <span className="text-xs text-muted-foreground">{turnCount} {t('test.turns')}</span>
            {status === 'streaming' && (
              <Button type="button" variant="ghost" size="sm" onClick={onStop}
                className="h-6 text-xs text-muted-foreground hover:text-foreground">
                <Square className="h-3 w-3 mr-1" fill="currentColor" />
                {t('test.stop')}
              </Button>
            )}
            <Button type="button" variant="ghost" size="sm" onClick={onClear}
              disabled={status === 'streaming'}
              className="h-6 text-xs text-muted-foreground hover:text-destructive">
              <Trash2 className="h-3 w-3 mr-1" />
              {t('test.clearConversation')}
            </Button>
          </div>
        )}
      </div>

      {/* Empty state */}
      {isEmpty && (
        <div className="flex-1 flex flex-col items-center justify-center py-8 text-muted-foreground text-sm gap-2">
          <p>{t('test.noSessions')}</p>
          <Button type="button" variant="outline" size="sm" onClick={onNewSession}>
            <Plus className="h-3.5 w-3.5 mr-1" />
            {t('test.newSession')}
          </Button>
        </div>
      )}

      {/* Message thread */}
      {!isEmpty && (
        <div ref={scrollRef} className="flex-1 overflow-y-auto p-3 space-y-3 min-h-0">
          {messages.map((msg, i) =>
            msg.role === 'system' ? (
              <div key={`msg-${i}-system`} className="flex items-center gap-2 py-1">
                <div className="flex-1 h-px bg-border/60" />
                <span className="text-[11px] text-muted-foreground/70 shrink-0">{msg.content}</span>
                <div className="flex-1 h-px bg-border/60" />
              </div>
            ) : msg.role === 'user' ? (
              <div key={`msg-${i}-user`} className="flex justify-end">
                <div className="max-w-[80%] rounded-2xl rounded-tr-sm px-3 py-2 bg-primary text-primary-foreground text-sm">
                  {msg.images && msg.images.length > 0 && (
                    <div className="flex gap-1 mb-2 flex-wrap">
                      {msg.images.map((b64, j) => (
                        // eslint-disable-next-line @next/next/no-img-element
                        <img key={b64.slice(0, 16)} src={`data:image/jpeg;base64,${b64}`} alt="" className="h-12 w-12 rounded object-cover" />
                      ))}
                    </div>
                  )}
                  <span className="whitespace-pre-wrap break-words">{msg.content}</span>
                </div>
              </div>
            ) : (
              <div key={`msg-${i}-assistant`} className="flex justify-start">
                <div className="max-w-[80%] relative group/msg">
                  {msg.model && (
                    <div className="mb-1 px-1">
                      <span className="text-xs text-muted-foreground/60 font-mono">{msg.model}</span>
                    </div>
                  )}
                  <div className="rounded-2xl rounded-tl-sm px-3 py-2 bg-muted text-foreground text-sm font-mono leading-relaxed">
                    {renderWithMermaid(msg.content, false)}
                  </div>
                  <div className="absolute top-1 right-1 opacity-0 group-hover/msg:opacity-100 transition-opacity">
                    <CopyButton text={msg.content} />
                  </div>
                </div>
              </div>
            )
          )}

          {status === 'streaming' && (
            <div className="flex justify-start">
              <div className="max-w-[80%] rounded-2xl rounded-tl-sm px-3 py-2 bg-muted text-foreground text-sm font-mono leading-relaxed">
                {streamingText
                  ? renderWithMermaid(streamingText, true)
                  : (
                    <span className="inline-flex flex-col gap-1">
                      <span className="inline-flex gap-1 items-center text-muted-foreground">
                        <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '0ms' }} />
                        <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '150ms' }} />
                        <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '300ms' }} />
                      </span>
                      {mcpToolCall && (
                        <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground/70 font-mono">
                          <Wrench className="h-3 w-3 shrink-0" />
                          {mcpToolCall}
                        </span>
                      )}
                    </span>
                  )
                }
              </div>
            </div>
          )}

          {status === 'error' && (
            <div className="rounded-md border border-status-error/30 bg-status-error/5 px-3 py-2 text-sm text-status-error-fg">
              {errorMsg}
            </div>
          )}

        </div>
      )}

      {/* Input area — hidden when no sessions */}
      {!isEmpty && (
        <div className="px-4 pt-3 pb-3 border-t border-border shrink-0">
          {/* Image thumbnails above input */}
          {images.length > 0 && (
            <div className="flex flex-wrap gap-2 mb-2">
              {images.map((b64, i) => (
                <div key={b64.slice(0, 16)} className="relative group">
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img
                    src={`data:image/jpeg;base64,${b64}`}
                    alt={`image-${i + 1}`}
                    className="h-12 w-12 rounded-md object-cover border border-border"
                  />
                  <button
                    type="button"
                    onClick={() => onImageRemove(i)}
                    aria-label={t('test.imageRemove')}
                    className="absolute -top-1.5 -right-1.5 hidden group-hover:flex h-4 w-4 items-center justify-center rounded-full bg-destructive text-destructive-foreground"
                  >
                    <X className="h-2.5 w-2.5" />
                  </button>
                </div>
              ))}
              {isCompressing && (
                <div className="flex h-12 w-12 items-center justify-center rounded-md border border-dashed border-border">
                  <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
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
            className="w-full rounded-md border-0 bg-transparent px-0 py-1 text-sm placeholder:text-muted-foreground focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 resize-none"
          />
          {/* Gmail-style bottom toolbar */}
          <div className="flex items-center gap-2 pt-2 border-t border-border/50">
            <Button
              type="button"
              onClick={onRun}
              disabled={!canRun}
              className="rounded-full px-5 h-8 text-sm font-medium"
              aria-label={t('test.run')}
            >
              <Send className="h-3.5 w-3.5 mr-1.5" />
              {t('test.run')}
            </Button>
            {!isGeminiProvider && (
              <>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  multiple
                  className="hidden"
                  onChange={handleFileChange}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  disabled={!canAddMore || isCompressing}
                  aria-label={t('test.imageAttach')}
                  title={t('test.imageAttach')}
                  className="h-8 w-8 text-muted-foreground hover:text-foreground"
                  onClick={() => fileInputRef.current?.click()}
                >
                  {isCompressing
                    ? <Loader2 className="h-4 w-4 animate-spin" />
                    : <ImagePlus className="h-4 w-4" />
                  }
                </Button>
              </>
            )}
            {!isGeminiProvider && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => onUseMcpChange(!useMcp)}
                title={useMcp ? 'MCP 비활성화' : 'MCP 활성화'}
                className={`h-8 px-2 text-xs gap-1 ${useMcp ? 'text-muted-foreground hover:text-foreground' : 'text-muted-foreground/40 hover:text-muted-foreground'}`}
              >
                <Wrench className="h-3.5 w-3.5" />
                MCP
              </Button>
            )}
            <span className="ml-auto text-xs text-muted-foreground/50">⌘↵</span>
          </div>
        </div>
      )}
    </div>
  )
})
