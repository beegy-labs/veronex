'use client'

import React, { useEffect, useRef } from 'react'

let _counter = 0

/** Renders a single Mermaid diagram from raw DSL code. */
export function MermaidBlock({ code }: { code: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const id = useRef(`mrd-${++_counter}`).current

  useEffect(() => {
    let cancelled = false
    const isDark = document.documentElement.classList.contains('dark')

    import('mermaid')
      .then(({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          theme: isDark ? 'dark' : 'default',
          securityLevel: 'loose',
        })
        return mermaid.render(id, code)
      })
      .then(({ svg }) => {
        if (!cancelled && ref.current) ref.current.innerHTML = svg
      })
      .catch(() => {
        // Fallback: display as plain code
        if (!cancelled && ref.current) {
          ref.current.textContent = code
        }
      })

    return () => { cancelled = true }
  }, [code, id])

  return (
    <div
      ref={ref}
      className="my-3 rounded-lg border border-border bg-card p-4 overflow-x-auto flex justify-center min-h-[80px]"
    />
  )
}

/**
 * Parse `text` and return ReactNodes where complete ```mermaid blocks are
 * rendered as diagrams and all other text is rendered as-is.
 * Incomplete blocks (still streaming) remain as plain text.
 */
export function renderWithMermaid(text: string, isStreaming: boolean): React.ReactNode {
  const parts = text.split(/(```mermaid\n[\s\S]*?\n```)/g)

  return (
    <>
      {parts.map((part, i) => {
        const match = part.match(/^```mermaid\n([\s\S]*?)\n```$/)
        if (match) {
          return <MermaidBlock key={i} code={match[1]} />
        }
        const isLast = i === parts.length - 1
        return (
          <span key={i} className="whitespace-pre-wrap">
            {part}
            {isLast && isStreaming && (
              <span className="inline-block w-0.5 h-4 bg-muted-foreground animate-pulse ml-px align-middle" />
            )}
          </span>
        )
      })}
    </>
  )
}
