'use client'

import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { builderApi } from '@/lib/builder-api'

export function BuilderTerminal({ workspaceId, cli }: { workspaceId: string; cli: string }) {
  const container = useRef<HTMLDivElement>(null)
  const [connection, setConnection] = useState('connecting')
  useEffect(() => {
    if (!container.current) return
    const terminal = new Terminal({ cursorBlink: true, convertEol: true, fontSize: 13 })
    const fit = new FitAddon()
    terminal.loadAddon(fit)
    terminal.open(container.current)
    fit.fit()
    const socket = new WebSocket(builderApi.terminalUrl(workspaceId))
    socket.binaryType = 'arraybuffer'
    socket.onopen = () => setConnection('connected')
    socket.onclose = () => setConnection('disconnected')
    socket.onerror = () => setConnection('error')
    socket.onmessage = (event) => terminal.write(event.data instanceof ArrayBuffer ? new Uint8Array(event.data) : String(event.data))
    const input = terminal.onData((data) => { if (socket.readyState === WebSocket.OPEN) socket.send(data) })
    const resize = new ResizeObserver(() => { fit.fit(); builderApi.resizeTerminal(workspaceId, terminal.rows, terminal.cols).catch(() => {}) })
    resize.observe(container.current)
    return () => { resize.disconnect(); input.dispose(); socket.close(); terminal.dispose() }
  }, [workspaceId, cli])
  return <section className="vds-space-y-2"><div className="vds-flex vds-justify-between"><h2 className="vds-text-lg vds-font-600">Terminal · {cli}</h2><span>{connection}</span></div>
    <div ref={container} className="vds-rounded-md vds-overflow-hidden" style={{ height: 460, background: '#111827' }} />
  </section>
}
