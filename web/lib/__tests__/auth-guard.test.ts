import { describe, it, expect } from 'vitest'
import { isPublicPath, PUBLIC_PATHS } from '../auth-guard'

describe('PUBLIC_PATHS', () => {
  it('contains /login and /setup', () => {
    expect(PUBLIC_PATHS).toContain('/login')
    expect(PUBLIC_PATHS).toContain('/setup')
  })

  it('has exactly 2 entries', () => {
    expect(PUBLIC_PATHS).toHaveLength(2)
  })
})

describe('isPublicPath', () => {
  it('returns true for public paths', () => {
    expect(isPublicPath('/login')).toBe(true)
    expect(isPublicPath('/setup')).toBe(true)
  })

  it('returns false for protected paths', () => {
    expect(isPublicPath('/overview')).toBe(false)
    expect(isPublicPath('/providers')).toBe(false)
    expect(isPublicPath('/keys')).toBe(false)
    expect(isPublicPath('/')).toBe(false)
  })

  it('is case-sensitive', () => {
    expect(isPublicPath('/Login')).toBe(false)
    expect(isPublicPath('/SETUP')).toBe(false)
  })

  it('does not match partial paths', () => {
    expect(isPublicPath('/login/extra')).toBe(false)
    expect(isPublicPath('/setup/')).toBe(false)
  })
})
