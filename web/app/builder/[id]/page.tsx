'use client'

import { use, useEffect, useState } from 'react'
import Link from 'next/link'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useLabSettings } from '@/components/lab-settings-provider'
import { builderApi, type BuilderWorkspace, type BuilderPreview, type BuilderEvent } from '@/lib/builder-api'
import { BuilderTerminal } from './terminal'

function PreviewFrame({ workspaceId, previewId, name }: { workspaceId: string; previewId: string; name: string }) {
  const [url, setUrl] = useState('')
  useEffect(() => {
    builderApi.previewTicket(workspaceId, previewId).then(({ url }) => setUrl(url)).catch(() => setUrl(''))
  }, [workspaceId, previewId])
  return <>{url && <><a href={url} target="_blank" rel="noreferrer">Open</a>
    <iframe title={`${name} preview`} src={url} sandbox="allow-scripts allow-forms allow-popups allow-same-origin"
      className="vds-w-full vds-rounded-md" style={{ height: 400 }} /></>}</>
}

export default function WorkspacePage({ params }: { params: Promise<{ id: string }> }) {
  usePageGuard('builder_manage')
  const { id } = use(params)
  const { labSettings } = useLabSettings()
  const [workspace, setWorkspace] = useState<BuilderWorkspace | null>(null)
  const [previews, setPreviews] = useState<BuilderPreview[]>([])
  const [previewName, setPreviewName] = useState('app')
  const [command, setCommand] = useState('npm run dev -- --host 0.0.0.0')
  const [port, setPort] = useState(3000)
  const [gitOutput, setGitOutput] = useState('')
  const [commitMessage, setCommitMessage] = useState('')
  const [pullTitle, setPullTitle] = useState('')
  const [pullBody, setPullBody] = useState('')
  const [pullUrl, setPullUrl] = useState('')
  const [previewLogs, setPreviewLogs] = useState<Record<string, string>>({})
  const [events, setEvents] = useState<BuilderEvent[]>([])
  const [error, setError] = useState('')
  async function reload() {
    const [w, p] = await Promise.all([builderApi.workspace(id), builderApi.previews(id)])
    setWorkspace(w); setPreviews(p.previews)
    const pull = await builderApi.pull(id).catch(() => ({ pull: null }))
    setPullUrl(pull.pull?.url ?? '')
  }
  useEffect(() => {
    if (!labSettings?.builder_enabled) return
    reload().catch((e) => setError(String(e)))
    const timer = setInterval(() => reload().catch(() => {}), 5000)
    return () => clearInterval(timer)
  }, [id, labSettings?.builder_enabled])
  async function run(action: () => Promise<unknown>) {
    try { await action(); setError(''); await reload() } catch (e) { setError(String(e)) }
  }
  if (!labSettings?.builder_enabled) return <p>App Builder is disabled.</p>
  if (!workspace) return <p>{error || 'Loading workspace…'}</p>
  return <main className="vds-space-y-6">
    <header className="vds-flex vds-justify-between"><div><Link href="/builder">← Workspaces</Link><h1 className="vds-text-2xl vds-font-700">{workspace.name}</h1><p>{workspace.branch} · {workspace.status}</p></div>
      <div className="vds-flex vds-gap-2">
        {(workspace.status === 'pending' || workspace.status === 'suspended') && <button onClick={() => run(() => builderApi.start(id))}>Start workspace Pod</button>}
        {(workspace.status === 'running' || workspace.status === 'starting' || workspace.status === 'failed') && <button onClick={() => run(() => builderApi.stop(id))}>Stop workspace Pod</button>}
      </div></header>
    {error && <p role="alert" className="vds-text-danger">{error}</p>}
    {workspace.last_error && <p role="alert">{workspace.last_error}</p>}
    {workspace.status === 'running' && <>
      <div className="vds-flex vds-gap-2 vds-items-center"><label htmlFor="cli-select">CLI</label>
        <select id="cli-select" value={workspace.cli} onChange={(e) => run(() => builderApi.switchCli(id, e.target.value))}>
          <option value="codex">Codex</option><option value="claude">Claude Code</option><option value="gemini">Gemini CLI</option>
        </select><span className="vds-text-dim">Handoff: /workspace/history/latest-handoff.md</span></div>
      <BuilderTerminal workspaceId={id} cli={workspace.cli} />
      <section className="vds-card vds-p-4 vds-space-y-2"><div className="vds-flex vds-justify-between"><h2 className="vds-text-lg vds-font-600">History</h2>
        <button onClick={() => builderApi.events(id).then((data) => setEvents(data.events)).catch((e) => setError(String(e)))}>Refresh</button></div>
        <div className="vds-overflow-auto" style={{ maxHeight: 260 }}>{events.map((event) => <details key={event.id} className="vds-border-b-1 vds-py-1">
          <summary>{event.cli} · {event.kind} · {new Date(event.created_at).toLocaleString()}</summary>
          <pre className="vds-text-xs vds-overflow-auto">{JSON.stringify(event.payload, null, 2)}</pre>
        </details>)}</div></section>
      <section className="vds-card vds-p-4 vds-space-y-3"><h2 className="vds-text-lg vds-font-600">Git</h2>
        <div className="vds-flex vds-gap-2"><button onClick={() => run(async () => setGitOutput(JSON.stringify(await builderApi.git(id, 'status'))))}>Status</button>
          <button onClick={() => run(async () => setGitOutput(JSON.stringify(await builderApi.git(id, 'fetch'))))}>Fetch</button>
          <input aria-label="Commit message" placeholder="Commit message" value={commitMessage} onChange={(e) => setCommitMessage(e.target.value)} />
          <button onClick={() => run(async () => setGitOutput(JSON.stringify(await builderApi.git(id, 'commit', commitMessage))))}>Commit</button>
          <button onClick={() => run(async () => setGitOutput(JSON.stringify(await builderApi.git(id, 'push'))))}>Push</button></div>
        {gitOutput && <pre className="vds-text-sm vds-overflow-auto">{gitOutput}</pre>}</section>
      <section className="vds-card vds-p-4 vds-space-y-3"><h2 className="vds-text-lg vds-font-600">Pull request</h2>
        {pullUrl ? <div className="vds-flex vds-gap-2"><a href={pullUrl} target="_blank" rel="noreferrer">Review pull request</a>
          <button onClick={() => run(() => builderApi.mergePull(id))}>Merge reviewed revision</button></div>
        : <form className="vds-flex vds-gap-2" onSubmit={(e) => { e.preventDefault(); run(() => builderApi.createPull(id, pullTitle, pullBody)) }}>
          <input aria-label="Pull request title" placeholder="Title" value={pullTitle} onChange={(e) => setPullTitle(e.target.value)} required />
          <input aria-label="Pull request body" placeholder="Description" value={pullBody} onChange={(e) => setPullBody(e.target.value)} />
          <button type="submit">Create pull request</button></form>}</section>
      <section className="vds-space-y-3"><h2 className="vds-text-lg vds-font-600">Previews</h2>
        <form className="vds-flex vds-gap-2" onSubmit={(e) => { e.preventDefault(); run(() => builderApi.startPreview(id, previewName, command, port)) }}>
          <input aria-label="Preview name" value={previewName} onChange={(e) => setPreviewName(e.target.value)} required />
          <input aria-label="Run command" className="vds-flex-1" value={command} onChange={(e) => setCommand(e.target.value)} required />
          <input aria-label="Port" type="number" min="1024" max="65535" value={port} onChange={(e) => setPort(Number(e.target.value))} required />
          <button type="submit">Run</button></form>
        <div className="vds-grid vds-grid-cols-2 vds-gap-4">{previews.map((preview) => <article key={preview.id} className="vds-card vds-p-3 vds-space-y-2">
          <div className="vds-flex vds-justify-between"><strong>{preview.name} · :{preview.port} · {preview.status}</strong>
            <div className="vds-flex vds-gap-2">
              <button onClick={() => builderApi.previewLogs(id, preview.id).then(({ logs }) => setPreviewLogs((previous) => ({ ...previous, [preview.id]: logs }))).catch((e) => setError(String(e)))}>Logs</button>
              {preview.status === 'running' && <button onClick={() => run(() => builderApi.stopPreview(id, preview.id))}>Stop</button>}</div></div>
          {previewLogs[preview.id] && <pre className="vds-text-xs vds-overflow-auto" style={{ maxHeight: 180 }}>{previewLogs[preview.id]}</pre>}
          {preview.status === 'running' && <PreviewFrame workspaceId={id} previewId={preview.id} name={preview.name} />}
        </article>)}</div></section>
    </>}
  </main>
}
