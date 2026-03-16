import { describe, it, expect } from 'vitest'
import {
  PROVIDER_BADGE, PROVIDER_COLORS, FINISH_COLORS, FINISH_BG,
  STATUS_STYLES, PROVIDER_STATUS_DOT, PROVIDER_STATUS_BADGE,
  PROVIDER_STATUS_TEXT, PROVIDER_STATUS_I18N, ROLE_STYLES,
  SUCCESS_RATE_GOOD, SUCCESS_RATE_WARNING, GPU_TEMP_CRITICAL,
  DEFAULT_MAX_IMAGES, MAX_IMAGES_LIMIT, DEFAULT_MAX_IMAGE_B64_BYTES,
  JOB_STATUS_COLORS,
} from '../constants'

describe('PROVIDER_BADGE', () => {
  it('has entries for ollama and gemini', () => {
    expect(PROVIDER_BADGE).toHaveProperty('ollama')
    expect(PROVIDER_BADGE).toHaveProperty('gemini')
  })
})

describe('PROVIDER_COLORS', () => {
  it('has CSS variable values for ollama and gemini', () => {
    expect(PROVIDER_COLORS.ollama).toContain('--theme-')
    expect(PROVIDER_COLORS.gemini).toContain('--theme-')
  })
})

describe('FINISH_COLORS', () => {
  it('has entries for stop, length, error, cancelled', () => {
    for (const key of ['stop', 'length', 'error', 'cancelled']) {
      expect(FINISH_COLORS).toHaveProperty(key)
    }
  })
})

describe('FINISH_BG', () => {
  it('has entries for stop, length, error, cancelled', () => {
    for (const key of ['stop', 'length', 'error', 'cancelled']) {
      expect(FINISH_BG).toHaveProperty(key)
    }
  })
})

describe('STATUS_STYLES', () => {
  it('has entries for all job statuses', () => {
    for (const status of ['completed', 'failed', 'cancelled', 'pending', 'running']) {
      expect(STATUS_STYLES).toHaveProperty(status)
    }
  })
})

describe('PROVIDER_STATUS_DOT', () => {
  it('has entries for online, degraded, offline', () => {
    for (const status of ['online', 'degraded', 'offline']) {
      expect(PROVIDER_STATUS_DOT).toHaveProperty(status)
    }
  })
})

describe('PROVIDER_STATUS_BADGE', () => {
  it('has entries for online, degraded, offline', () => {
    for (const status of ['online', 'degraded', 'offline']) {
      expect(PROVIDER_STATUS_BADGE).toHaveProperty(status)
    }
  })
})

describe('PROVIDER_STATUS_TEXT', () => {
  it('has entries for online, degraded, offline', () => {
    for (const status of ['online', 'degraded', 'offline']) {
      expect(PROVIDER_STATUS_TEXT).toHaveProperty(status)
    }
  })
})

describe('PROVIDER_STATUS_I18N', () => {
  it('maps statuses to i18n keys', () => {
    expect(PROVIDER_STATUS_I18N.online).toBe('common.online')
    expect(PROVIDER_STATUS_I18N.degraded).toBe('common.degraded')
    expect(PROVIDER_STATUS_I18N.offline).toBe('common.offline')
  })
})

describe('ROLE_STYLES', () => {
  it('has entries for system, user, assistant, tool', () => {
    for (const role of ['system', 'user', 'assistant', 'tool']) {
      expect(ROLE_STYLES).toHaveProperty(role)
    }
  })
})

describe('Threshold constants', () => {
  it('SUCCESS_RATE_GOOD is 90', () => {
    expect(SUCCESS_RATE_GOOD).toBe(90)
  })

  it('SUCCESS_RATE_WARNING is 70', () => {
    expect(SUCCESS_RATE_WARNING).toBe(70)
  })

  it('GPU_TEMP_CRITICAL is 85', () => {
    expect(GPU_TEMP_CRITICAL).toBe(85)
  })

  it('SUCCESS_RATE_GOOD > SUCCESS_RATE_WARNING (ordering invariant)', () => {
    expect(SUCCESS_RATE_GOOD).toBeGreaterThan(SUCCESS_RATE_WARNING)
  })
})

describe('Image limit constants', () => {
  it('DEFAULT_MAX_IMAGES is 4', () => {
    expect(DEFAULT_MAX_IMAGES).toBe(4)
  })

  it('MAX_IMAGES_LIMIT is 20', () => {
    expect(MAX_IMAGES_LIMIT).toBe(20)
  })

  it('DEFAULT_MAX_IMAGE_B64_BYTES is 2 MiB', () => {
    expect(DEFAULT_MAX_IMAGE_B64_BYTES).toBe(2 * 1024 * 1024)
  })

  it('DEFAULT_MAX_IMAGES <= MAX_IMAGES_LIMIT', () => {
    expect(DEFAULT_MAX_IMAGES).toBeLessThanOrEqual(MAX_IMAGES_LIMIT)
  })
})

describe('JOB_STATUS_COLORS', () => {
  it('has entries for all 5 job statuses', () => {
    for (const status of ['completed', 'failed', 'cancelled', 'pending', 'running']) {
      expect(JOB_STATUS_COLORS).toHaveProperty(status)
    }
  })
})
