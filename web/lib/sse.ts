/**
 * SSE (Server-Sent Events) stream parsing utilities.
 * Shared by api-test-panel (single-run and conversation modes).
 */

export interface SseLine {
  /** Value of the most recent `event:` field, or '' if none. */
  eventType: string
  /** Content of the `data:` field (leading space stripped). */
  data: string
}

/**
 * Async generator that reads a raw SSE byte stream and yields one {eventType, data}
 * object per `data:` line. Tracks `event:` fields and resets them on blank lines.
 *
 * Callers are responsible for handling `[DONE]` and error event types.
 */
export async function* iterSseLines(
  reader: ReadableStreamDefaultReader<Uint8Array>,
): AsyncGenerator<SseLine> {
  const decoder = new TextDecoder()
  let buf = ''
  let eventType = ''

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buf += decoder.decode(value, { stream: true })
    const lines = buf.split('\n')
    buf = lines.pop() ?? ''
    for (const line of lines) {
      const trimmed = line.trimEnd()
      if (trimmed === '') { eventType = ''; continue }
      if (trimmed.startsWith('event:')) { eventType = trimmed.slice(6).trim(); continue }
      if (!trimmed.startsWith('data:')) continue
      const raw = trimmed.slice(5)
      const data = raw.startsWith(' ') ? raw.slice(1) : raw
      yield { eventType, data }
    }
  }
}
