export interface OpenAIChunk {
  id?: string
  choices?: { delta?: { content?: string }; finish_reason?: string | null }[]
  error?: { message?: string }
}

export type ProviderOption = { value: string; label: string; isGemini: boolean }
export type StreamStatus = 'idle' | 'streaming' | 'done' | 'error'
export type Endpoint = '/v1/chat/completions' | '/api/chat' | '/api/generate' | '/v1beta/models'

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

export interface ConversationMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
  images?: string[] // base64, user messages only
  model?: string    // model used for this turn (assistant messages only)
}

export interface ConversationSession {
  id: number
  messages: ConversationMessage[]
  streamingText: string
  status: 'idle' | 'streaming' | 'error'
  errorMsg: string
  conversationId?: string  // server-assigned conversation ID (base62 UUID)
}

export const MAX_CONV_SESSIONS = 10

export type TestMode = 'single' | 'conversation'
