import { describe, it, expect } from 'vitest'
import { fmtCompact, fmtMs, fmtMsAxis, fmtMsNullable, fmtPct, fmtMb, fmtMbShort } from '../chart-theme'

describe('fmtCompact', () => {
  it('returns raw number below 1K', () => {
    expect(fmtCompact(0)).toBe('0')
    expect(fmtCompact(999)).toBe('999')
  })

  it('formats thousands as K', () => {
    expect(fmtCompact(1_000)).toBe('1.0K')
    expect(fmtCompact(1_500)).toBe('1.5K')
    expect(fmtCompact(999_999)).toBe('1000.0K')
  })

  it('formats millions as M', () => {
    expect(fmtCompact(1_000_000)).toBe('1.0M')
    expect(fmtCompact(2_500_000)).toBe('2.5M')
  })
})

describe('fmtMs', () => {
  it('formats sub-second as ms', () => {
    expect(fmtMs(0)).toBe('0ms')
    expect(fmtMs(543)).toBe('543ms')
    expect(fmtMs(999)).toBe('999ms')
  })

  it('formats seconds', () => {
    expect(fmtMs(1_000)).toBe('1.0s')
    expect(fmtMs(1_400)).toBe('1.4s')
    expect(fmtMs(59_999)).toBe('60.0s')
  })

  it('formats minutes and seconds', () => {
    expect(fmtMs(60_000)).toBe('1m')
    expect(fmtMs(86_000)).toBe('1m 26s')
  })

  it('formats hours and minutes', () => {
    expect(fmtMs(3_600_000)).toBe('1h')
    expect(fmtMs(5_400_000)).toBe('1h 30m')
  })
})

describe('fmtMsAxis', () => {
  it('formats sub-second as ms', () => {
    expect(fmtMsAxis(543)).toBe('543ms')
  })

  it('formats seconds without decimals', () => {
    expect(fmtMsAxis(1_400)).toBe('1s')
    expect(fmtMsAxis(30_000)).toBe('30s')
  })

  it('formats minutes with one decimal', () => {
    expect(fmtMsAxis(86_360)).toBe('1.4m')
  })

  it('formats hours with one decimal', () => {
    expect(fmtMsAxis(5_400_000)).toBe('1.5h')
  })
})

describe('fmtMsNullable', () => {
  it('returns dash for null/undefined', () => {
    expect(fmtMsNullable(null)).toBe('—')
    expect(fmtMsNullable(undefined)).toBe('—')
  })

  it('delegates to fmtMs for numbers', () => {
    expect(fmtMsNullable(543)).toBe('543ms')
    expect(fmtMsNullable(1_400)).toBe('1.4s')
  })
})

describe('fmtPct', () => {
  it('formats percentage value', () => {
    expect(fmtPct(0)).toBe('0%')
    expect(fmtPct(50)).toBe('50%')
    expect(fmtPct(95.6)).toBe('96%')
    expect(fmtPct(100)).toBe('100%')
  })
})

describe('fmtMb', () => {
  it('formats MiB below 1024', () => {
    expect(fmtMb(512)).toBe('512 MiB')
  })

  it('formats GiB at or above 1024', () => {
    expect(fmtMb(1024)).toBe('1.0 GiB')
    expect(fmtMb(2048)).toBe('2.0 GiB')
    expect(fmtMb(8192)).toBe('8.0 GiB')
  })
})

describe('fmtMbShort', () => {
  it('returns dash for zero', () => {
    expect(fmtMbShort(0)).toBe('—')
  })

  it('formats MB below 1024', () => {
    expect(fmtMbShort(512)).toBe('512 MB')
  })

  it('formats GB at or above 1024', () => {
    expect(fmtMbShort(1024)).toBe('1.0 GB')
    expect(fmtMbShort(2048)).toBe('2.0 GB')
    expect(fmtMbShort(8192)).toBe('8.0 GB')
  })
})
