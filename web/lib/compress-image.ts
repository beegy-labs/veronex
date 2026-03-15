import imageCompression from 'browser-image-compression'

/**
 * Compress an image file and return raw base64 (no data URL prefix).
 *
 * Ollama requires raw base64 — passing a data URL prefix such as
 * "data:image/jpeg;base64," causes a decode error on the Ollama side.
 *
 * Uses browser-image-compression with useWebWorker: true so compression
 * runs off the main thread and does not block the UI.
 */
export async function compressImage(
  file: File,
  maxDim = 1024,   // safe upper bound for all Ollama vision models
  quality = 0.85,  // JPEG quality (0.82–0.90 range preserves AI inference accuracy)
): Promise<string> {
  const compressed = await imageCompression(file, {
    maxSizeMB: 1.5,
    maxWidthOrHeight: maxDim,
    useWebWorker: true,
    fileType: 'image/jpeg',
    initialQuality: quality,
  })

  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const dataUrl = reader.result as string
      // Strip "data:image/jpeg;base64," prefix — Ollama requires raw base64
      resolve(dataUrl.split(',')[1])
    }
    reader.onerror = reject
    reader.readAsDataURL(compressed)
  })
}
