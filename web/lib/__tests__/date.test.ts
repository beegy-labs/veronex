import { describe, it, expect } from 'vitest'
import { fmtDatetime, fmtDatetimeShort, fmtDateOnly, fmtNumber, fmtHourLabel } from '../date'

describe('fmtNumber', () => {
  it('formats integers with comma separators', () => {
    expect(fmtNumber(0)).toBe('0')
    expect(fmtNumber(999)).toBe('999')
    expect(fmtNumber(1_000)).toBe('1,000')
    expect(fmtNumber(1_234_567)).toBe('1,234,567')
  })
})

describe('fmtHourLabel', () => {
  it('formats ISO date to "M/D HHh" in UTC', () => {
    const result = fmtHourLabel('2026-03-08T14:00:00Z', 'UTC')
    expect(result).toBe('3/8 14h')
  })

  it('applies timezone offset', () => {
    // UTC midnight → KST 09:00
    const result = fmtHourLabel('2026-03-08T00:00:00Z', 'Asia/Seoul')
    expect(result).toBe('3/8 09h')
  })

  it('handles date rollover across timezone', () => {
    // UTC 23:00 Mar 8 → KST 08:00 Mar 9
    const result = fmtHourLabel('2026-03-08T23:00:00Z', 'Asia/Seoul')
    expect(result).toBe('3/9 08h')
  })
})

describe('fmtDatetime', () => {
  it('returns a string containing month, day, and time', () => {
    const result = fmtDatetime('2026-03-08T14:30:45Z', 'UTC')
    expect(result).toContain('8')
    expect(result).toMatch(/14|2:30/) // 24h or 12h depending on locale
  })
})

describe('fmtDatetimeShort', () => {
  it('returns a string without seconds', () => {
    const result = fmtDatetimeShort('2026-03-08T14:30:45Z', 'UTC')
    expect(result).toContain('8')
    // Should not contain ":45" (seconds)
    expect(result).not.toContain(':45')
  })
})

describe('fmtDateOnly', () => {
  it('returns date with year but no time component', () => {
    const result = fmtDateOnly('2026-03-08T14:30:45Z', 'UTC')
    expect(result).toContain('2026')
    expect(result).toContain('8')
  })
})
