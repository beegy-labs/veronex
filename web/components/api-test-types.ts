export interface OpenAIChunk {
  id?: string
  choices?: { delta?: { content?: string }; finish_reason?: string | null }[]
  error?: { message?: string }
}

export type ProviderOption = { value: string; label: string; isGemini: boolean }
export type StreamStatus = 'idle' | 'streaming' | 'done' | 'error'
export type Endpoint = '/v1/chat/completions' | '/api/chat' | '/api/generate'

export interface Run {
  id: number
  prompt: string
  model: string
  provider_type: string
  endpoint: Endpoint
  useApiKey: boolean
  status: StreamStatus
  text: string
  errorMsg: string
  images?: string[]  // raw base64 (no data URL prefix)
}

export type RunAction =
  | { type: 'APPEND'; id: number; token: string }
  | { type: 'SET_STATUS'; id: number; status: StreamStatus; errorMsg?: string }
  | { type: 'ADD'; run: Run }
  | { type: 'REMOVE'; id: number }

export function runsReducer(state: Run[], action: RunAction): Run[] {
  switch (action.type) {
    case 'ADD':
      return [...state, action.run]
    case 'REMOVE':
      return state.filter((r) => r.id !== action.id)
    case 'APPEND':
      return state.map((r) =>
        r.id === action.id ? { ...r, text: r.text + action.token } : r
      )
    case 'SET_STATUS':
      return state.map((r) =>
        r.id === action.id
          ? { ...r, status: action.status, errorMsg: action.errorMsg ?? r.errorMsg }
          : r
      )
    default:
      return state
  }
}

export const MAX_RUNS = 10
