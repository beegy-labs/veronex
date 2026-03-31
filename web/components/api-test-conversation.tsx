'use client'

import { useEffect, useRef } from 'react'
import { Trash2, Square } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/i18n'
import { CopyButton } from '@/components/copy-button'
import { renderWithMermaid } from '@/components/mermaid-block'
import type { ConversationMessage, StreamStatus } from '@/components/api-test-types'

interface ApiTestConversationProps {
  messages: ConversationMessage[]
  streamingText: string
  status: StreamStatus
  errorMsg: string
  onClear: () => void
  onStop: () => void
}

export function ApiTestConversation({
  messages, streamingText, status, errorMsg, onClear, onStop,
}: ApiTestConversationProps) {
  const { t } = useTranslation()
  const endRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages.length, streamingText])

  if (messages.length === 0 && status === 'idle') return null

  const turnCount = messages.filter((m) => m.role === 'user').length

  return (
    <div className="border border-border rounded-md overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-border bg-muted/30">
        <span className="text-xs text-muted-foreground">
          {turnCount} {t('test.turns')}
        </span>
        <div className="flex items-center gap-1">
          {status === 'streaming' && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={onStop}
              className="h-6 text-xs text-muted-foreground hover:text-foreground"
            >
              <Square className="h-3 w-3 mr-1" fill="currentColor" />
              {t('test.stop')}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClear}
            disabled={status === 'streaming'}
            className="h-6 text-xs text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-3 w-3 mr-1" />
            {t('test.clearConversation')}
          </Button>
        </div>
      </div>

      {/* Message thread */}
      <div className="max-h-96 overflow-y-auto p-3 space-y-3">
        {messages.map((msg, i) =>
          msg.role === 'user' ? (
            <div key={i} className="flex justify-end">
              <div className="max-w-[80%] rounded-2xl rounded-tr-sm px-3 py-2 bg-primary text-primary-foreground text-sm">
                {msg.images && msg.images.length > 0 && (
                  <div className="flex gap-1 mb-2 flex-wrap">
                    {msg.images.map((b64, j) => (
                      // eslint-disable-next-line @next/next/no-img-element
                      <img
                        key={j}
                        src={`data:image/jpeg;base64,${b64}`}
                        alt=""
                        className="h-12 w-12 rounded object-cover"
                      />
                    ))}
                  </div>
                )}
                <span className="whitespace-pre-wrap break-words">{msg.content}</span>
              </div>
            </div>
          ) : (
            <div key={i} className="flex justify-start">
              <div className="max-w-[80%] relative group/msg">
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

        {/* Streaming response */}
        {status === 'streaming' && (
          <div className="flex justify-start">
            <div className="max-w-[80%] rounded-2xl rounded-tl-sm px-3 py-2 bg-muted text-foreground text-sm font-mono leading-relaxed">
              {streamingText
                ? renderWithMermaid(streamingText, true)
                : <span className="inline-flex gap-1 items-center text-muted-foreground">
                    <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '0ms' }} />
                    <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '150ms' }} />
                    <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground animate-bounce" style={{ animationDelay: '300ms' }} />
                  </span>
              }
            </div>
          </div>
        )}

        {/* Error */}
        {status === 'error' && (
          <div className="rounded-md border border-status-error/30 bg-status-error/5 px-3 py-2 text-sm text-status-error-fg">
            {errorMsg}
          </div>
        )}

        <div ref={endRef} />
      </div>
    </div>
  )
}
