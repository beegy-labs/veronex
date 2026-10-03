'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import { useRouter } from 'next/navigation'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useLabSettings } from '@/components/lab-settings-provider'
import { builderApi, type BuilderRepository, type BuilderWorkspace } from '@/lib/builder-api'

export default function BuilderPage() {
  usePageGuard('builder_manage')
  const { labSettings } = useLabSettings()
  const router = useRouter()
  const [repositories, setRepositories] = useState<BuilderRepository[]>([])
  const [workspaces, setWorkspaces] = useState<BuilderWorkspace[]>([])
  const [name, setName] = useState('')
  const [remote, setRemote] = useState('')
  const [branch, setBranch] = useState('main')
  const [authorName, setAuthorName] = useState('')
  const [authorEmail, setAuthorEmail] = useState('')
  const [repoId, setRepoId] = useState('')
  const [workspaceName, setWorkspaceName] = useState('')
  const [error, setError] = useState('')

  async function reload() {
    const [repoData, workspaceData] = await Promise.all([builderApi.repositories(), builderApi.workspaces()])
    setRepositories(repoData.repositories)
    setWorkspaces(workspaceData.workspaces)
    setRepoId((current) => current || repoData.repositories[0]?.id || '')
  }
  useEffect(() => { if (labSettings?.builder_enabled) reload().catch((e) => setError(String(e))) }, [labSettings?.builder_enabled])

  if (!labSettings?.builder_enabled) return <p className="vds-p-4">App Builder is disabled in Lab settings.</p>

  return <main className="vds-space-y-6">
    <header><h1 className="vds-text-2xl vds-font-700">App Builder</h1><p className="vds-text-dim">Remote repositories and persistent workspaces</p></header>
    {error && <p role="alert" className="vds-text-danger">{error}</p>}
    <section className="vds-card vds-p-4 vds-space-y-3">
      <h2 className="vds-text-lg vds-font-600">Connect repository</h2>
      <form className="vds-flex vds-gap-2" onSubmit={async (event) => { event.preventDefault(); try { await builderApi.createRepository(name, remote, branch, authorName, authorEmail); setError(''); await reload() } catch (e) { setError(String(e)) } }}>
        <input aria-label="Repository name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} required />
        <input aria-label="HTTPS Git URL" placeholder="https://git.example.com/owner/repo.git" value={remote} onChange={(e) => setRemote(e.target.value)} required className="vds-flex-1" />
        <input aria-label="Default branch" value={branch} onChange={(e) => setBranch(e.target.value)} required />
        <input aria-label="Git author name" placeholder="Author name" value={authorName} onChange={(e) => setAuthorName(e.target.value)} required />
        <input aria-label="Git author email" type="email" placeholder="Author email" value={authorEmail} onChange={(e) => setAuthorEmail(e.target.value)} required />
        <button type="submit">Connect</button>
      </form>
    </section>
    <section className="vds-card vds-p-4 vds-space-y-3">
      <h2 className="vds-text-lg vds-font-600">New workspace</h2>
      <form className="vds-flex vds-gap-2" onSubmit={async (event) => { event.preventDefault(); try { const row = await builderApi.createWorkspace(repoId, workspaceName, `builder/${workspaceName}`, 'codex'); router.push(`/builder/${row.id}`) } catch (e) { setError(String(e)) } }}>
        <select aria-label="Repository" value={repoId} onChange={(e) => setRepoId(e.target.value)} required>{repositories.map((repo) => <option key={repo.id} value={repo.id}>{repo.name}</option>)}</select>
        <input aria-label="Workspace name" placeholder="Workspace name" value={workspaceName} onChange={(e) => setWorkspaceName(e.target.value)} required />
        <button type="submit" disabled={!repoId}>Create</button>
      </form>
    </section>
    <section className="vds-space-y-2"><h2 className="vds-text-lg vds-font-600">Workspaces</h2>
      {workspaces.map((workspace) => <Link key={workspace.id} href={`/builder/${workspace.id}`} className="vds-card vds-p-4 vds-flex vds-justify-between"><span>{workspace.name} <small>{workspace.branch}</small></span><span>{workspace.cli} · {workspace.status}</span></Link>)}
    </section>
  </main>
}
