import { apiClient } from './api-client'
import { BASE_API_URL } from './constants'

export interface BuilderRepository {
  id: string
  name: string
  remote_url: string
  author_name: string
  author_email: string
  default_branch: string
}

export interface BuilderWorkspace {
  id: string
  repository_id: string
  name: string
  branch: string
  cli: 'codex' | 'claude' | 'gemini' | 'local'
  status: string
  pod_ip: string | null
  last_error: string | null
}

export interface BuilderPreview {
  id: string
  name: string
  command: string
  port: number
  status: string
  last_error: string | null
}

export interface BuilderEvent {
  id: number
  cli: string
  kind: string
  payload: unknown
  created_at: string
}

const root = '/v1/builder'
export const builderApi = {
  repositories: () => apiClient.get<{ repositories: BuilderRepository[] }>(`${root}/repositories`),
  createRepository: (name: string, remote_url: string, default_branch: string, author_name: string, author_email: string) =>
    apiClient.post<BuilderRepository>(`${root}/repositories`, { name, remote_url, default_branch, author_name, author_email }),
  workspaces: () => apiClient.get<{ workspaces: BuilderWorkspace[] }>(`${root}/workspaces`),
  workspace: (id: string) => apiClient.get<BuilderWorkspace>(`${root}/workspaces/${id}`),
  createWorkspace: (repository_id: string, name: string, branch: string, cli: string) =>
    apiClient.post<BuilderWorkspace>(`${root}/workspaces`, { repository_id, name, branch, cli }),
  start: (id: string) => apiClient.post<BuilderWorkspace>(`${root}/workspaces/${id}/start`),
  stop: (id: string) => apiClient.post<BuilderWorkspace>(`${root}/workspaces/${id}/stop`),
  switchCli: (id: string, cli: string) => apiClient.post(`${root}/workspaces/${id}/switch`, { cli }),
  git: (id: string, action: string, message?: string) =>
    apiClient.post<{ stdout: string; stderr: string }>(`${root}/workspaces/${id}/git/${action}`, { message }),
  pull: (id: string) => apiClient.get<{ pull: { url?: string; status?: string; remote?: unknown } | null }>(`${root}/workspaces/${id}/pull`),
  events: (id: string) => apiClient.get<{ events: BuilderEvent[] }>(`${root}/workspaces/${id}/events`),
  createPull: (id: string, title: string, body: string) =>
    apiClient.post(`${root}/workspaces/${id}/pull`, { title, body }),
  mergePull: (id: string) => apiClient.post(`${root}/workspaces/${id}/pull/merge`),
  previews: (id: string) => apiClient.get<{ previews: BuilderPreview[] }>(`${root}/workspaces/${id}/previews`),
  startPreview: (id: string, name: string, command: string, port: number) =>
    apiClient.post<BuilderPreview>(`${root}/workspaces/${id}/previews`, { name, command, port }),
  stopPreview: (id: string, previewId: string) =>
    apiClient.post(`${root}/workspaces/${id}/previews/${previewId}/stop`),
  previewLogs: (id: string, previewId: string) =>
    apiClient.get<{ logs: string }>(`${root}/workspaces/${id}/previews/${previewId}/logs`),
  previewTicket: (id: string, previewId: string) =>
    apiClient.post<{ url: string }>(`${root}/workspaces/${id}/previews/${previewId}/ticket`),
  previewUrl: (id: string, previewId: string) =>
    `${BASE_API_URL}${root}/workspaces/${id}/previews/${previewId}/view`,
  terminalUrl: (id: string) =>
    `${BASE_API_URL.replace(/^http/, 'ws')}${root}/workspaces/${id}/terminal`,
  resizeTerminal: (id: string, rows: number, cols: number) =>
    apiClient.post(`${root}/workspaces/${id}/terminal/resize`, { rows, cols }),
}
