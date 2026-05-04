export interface OpenAIChunk {
  id?: string
  choices?: {
    delta?: {
      content?: string
      tool_calls?: { function?: { name?: string; arguments?: string } }[]
    }
    finish_reason?: string | null
  }[]
  error?: { message?: string }
}

export type ProviderOption = { value: string; label: string; isGemini: boolean }
export type StreamStatus = 'idle' | 'streaming' | 'done' | 'error'
export type Endpoint = '/v1/chat/completions' | '/api/chat' | '/api/generate' | '/v1beta/models'

/// One MCP tool invocation observed during streaming.
/// SDD: `.specs/veronex/inference-mcp-streaming-first.md` §7.
export interface McpToolCall {
  /// Tool function name (e.g. "mcp_..._web_search")
  name: string
  /// Server-side timestamp when first observed (ms since epoch).
  startedAt: number
}

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
  /// Tool calls observed in the SSE stream — append-only timeline rendered
  /// above the result text panel. Empty for non-MCP runs.
  toolCalls: McpToolCall[]
}

export type RunAction =
  | { type: 'APPEND'; id: number; token: string }
  | { type: 'TOOL_CALL'; id: number; name: string }
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
    case 'TOOL_CALL':
      return state.map((r) => {
        if (r.id !== action.id) return r
        // Idempotent: drop duplicate consecutive entries (OpenAI streams
        // tool_call name once on the first delta and arguments incrementally
        // after — only the name event is timeline-relevant).
        const last = r.toolCalls[r.toolCalls.length - 1]
        if (last && last.name === action.name) return r
        return {
          ...r,
          toolCalls: [...r.toolCalls, { name: action.name, startedAt: Date.now() }],
        }
      })
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
  /**
   * Job UUID extracted from SSE chunk id (`chatcmpl-mcp-<uuid>`). Set on
   * assistant messages after the stream completes; used by the conversation
   * UI to fetch the per-turn MCP tool-call audit via
   * `GET /v1/conversations/{id}/turns/{job_id}/internals`. SDD:
   * `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md`.
   */
  jobId?: string
  /**
   * True when this turn invoked any MCP tool. Drives the inline
   * `<ToolCallTimeline>` reveal in the conversation UI. Even when the model
   * never produced text, this flag lets the UI show the user what the system
   * actually did (search queries, results, latency).
   */
  hasMcpTools?: boolean
  /**
   * Model-emitted tool_calls captured from the S3 TurnRecord after the
   * stream completes. Renders inline below the assistant bubble so the
   * user sees the chain (tool name + args) for tool-only turns.
   * SDD `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md`.
   */
  toolCalls?: { name: string; arguments: unknown }[]
}

export interface ConversationSession {
  id: number
  messages: ConversationMessage[]
  streamingText: string
  status: 'idle' | 'streaming' | 'error'
  errorMsg: string
  conversationId?: string  // server-assigned conversation ID (base62 UUID)
  mcpToolCall?: string     // currently executing MCP tool name (cleared when content arrives)
}

export const MAX_CONV_SESSIONS = 10

export type TestMode = 'single' | 'conversation'
