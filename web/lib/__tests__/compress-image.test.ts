import { describe, it, expect, vi } from 'vitest'

// Mock browser-image-compression — returns a Blob with known content
vi.mock('browser-image-compression', () => ({
  default: vi.fn(async () => new Blob(['fake-jpeg'], { type: 'image/jpeg' })),
}))

// Mock FileReader since we're in Node environment
class MockFileReader {
  result: string | null = null
  onload: (() => void) | null = null
  onerror: ((e: unknown) => void) | null = null

  readAsDataURL(_blob: Blob) {
    // Simulate base64 encoding with data URL prefix
    this.result = 'data:image/jpeg;base64,aGVsbG8=' // "hello" in base64
    setTimeout(() => this.onload?.(), 0)
  }
}

vi.stubGlobal('FileReader', MockFileReader)

import { compressImage } from '../compress-image'

describe('compressImage', () => {
  it('returns raw base64 without data URL prefix', async () => {
    const file = new File(['test'], 'test.jpg', { type: 'image/jpeg' })
    const result = await compressImage(file)

    // Must NOT contain "data:image/jpeg;base64," prefix
    expect(result).not.toContain('data:')
    expect(result).not.toContain(';base64,')
    expect(result).toBe('aGVsbG8=')
  })

  it('calls browser-image-compression with correct options', async () => {
    const { default: mockCompress } = await import('browser-image-compression')
    const file = new File(['test'], 'photo.png', { type: 'image/png' })

    await compressImage(file, 512, 0.9)

    expect(mockCompress).toHaveBeenCalledWith(file, {
      maxSizeMB: 1.5,
      maxWidthOrHeight: 512,
      useWebWorker: true,
      fileType: 'image/jpeg',
      initialQuality: 0.9,
    })
  })

  it('uses default maxDim=1024 and quality=0.85', async () => {
    const { default: mockCompress } = await import('browser-image-compression')
    ;(mockCompress as ReturnType<typeof vi.fn>).mockClear()

    const file = new File(['test'], 'img.jpg', { type: 'image/jpeg' })
    await compressImage(file)

    expect(mockCompress).toHaveBeenCalledWith(file, expect.objectContaining({
      maxWidthOrHeight: 1024,
      initialQuality: 0.85,
    }))
  })
})
