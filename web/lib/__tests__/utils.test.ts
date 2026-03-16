import { describe, it, expect } from 'vitest'
import { getOllamaProviders, getGeminiProviders, countByStatus, calcPercentage, successRateCls } from '../utils'
import type { Provider } from '../types'

// Minimal Provider stub — only fields used by the filter functions
function makeProvider(type: 'ollama' | 'gemini'): Provider {
  return { provider_type: type } as Provider
}

describe('getOllamaProviders', () => {
  it('returns only ollama providers', () => {
    const providers = [makeProvider('ollama'), makeProvider('gemini'), makeProvider('ollama')]
    const filtered = getOllamaProviders(providers)
    expect(filtered).toHaveLength(2)
    expect(filtered.every(p => p.provider_type === 'ollama')).toBe(true)
  })

  it('returns empty array when none match', () => {
    expect(getOllamaProviders([makeProvider('gemini')])).toEqual([])
  })

  it('returns empty array for undefined input', () => {
    expect(getOllamaProviders(undefined)).toEqual([])
  })
})

describe('getGeminiProviders', () => {
  it('returns only gemini providers', () => {
    const providers = [makeProvider('ollama'), makeProvider('gemini')]
    expect(getGeminiProviders(providers)).toHaveLength(1)
    expect(getGeminiProviders(providers)[0].provider_type).toBe('gemini')
  })

  it('returns empty array for undefined input', () => {
    expect(getGeminiProviders(undefined)).toEqual([])
  })
})

describe('countByStatus', () => {
  it('returns empty object for empty array', () => {
    expect(countByStatus([])).toEqual({})
  })

  it('counts each status', () => {
    const items = [
      { status: 'completed' },
      { status: 'failed' },
      { status: 'completed' },
      { status: 'running' },
    ]
    expect(countByStatus(items)).toEqual({ completed: 2, failed: 1, running: 1 })
  })

  it('handles single-status input', () => {
    expect(countByStatus([{ status: 'pending' }, { status: 'pending' }])).toEqual({ pending: 2 })
  })
})

describe('calcPercentage', () => {
  it('returns 0 for zero denominator (no division by zero)', () => {
    expect(calcPercentage(5, 0)).toBe(0)
  })

  it('calculates whole percentage', () => {
    expect(calcPercentage(1, 4)).toBe(25)
    expect(calcPercentage(100, 100)).toBe(100)
  })

  it('rounds to nearest integer', () => {
    expect(calcPercentage(1, 3)).toBe(33)
    expect(calcPercentage(2, 3)).toBe(67)
  })

  it('returns 0 for 0 numerator', () => {
    expect(calcPercentage(0, 100)).toBe(0)
  })
})

describe('successRateCls', () => {
  it('returns muted class for undefined rate', () => {
    expect(successRateCls(undefined)).toBe('text-muted-foreground')
  })

  it('returns success class for rate >= 90', () => {
    expect(successRateCls(90)).toContain('status-success')
    expect(successRateCls(100)).toContain('status-success')
    expect(successRateCls(95.5)).toContain('status-success')
  })

  it('boundary: exactly 90 is success (not warning)', () => {
    expect(successRateCls(90)).toContain('status-success')
    expect(successRateCls(90)).not.toContain('status-warning')
  })

  it('returns warning class for rate >= 70 and < 90', () => {
    expect(successRateCls(70)).toContain('status-warning')
    expect(successRateCls(80)).toContain('status-warning')
    expect(successRateCls(89.9)).toContain('status-warning')
  })

  it('boundary: exactly 70 is warning (not error)', () => {
    expect(successRateCls(70)).toContain('status-warning')
    expect(successRateCls(70)).not.toContain('status-error')
  })

  it('returns error class for rate < 70', () => {
    expect(successRateCls(69.9)).toContain('status-error')
    expect(successRateCls(0)).toContain('status-error')
  })
})
