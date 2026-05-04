'use client'

import { useState, useCallback } from 'react'

/**
 * Shared drag-and-drop image handler for components that accept image uploads.
 * Filters non-image files and gates on canAddMore before accepting drops.
 */
export function useImageDrop(canAddMore: boolean, onImageAdd: (files: FileList) => void) {
  const [isDragging, setIsDragging] = useState(false)

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    if (canAddMore) setIsDragging(true)
  }, [canAddMore])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }, [])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
    if (!canAddMore) return
    const files = e.dataTransfer.files
    if (files.length > 0) {
      const imageFiles = Array.from(files).filter((f) => f.type.startsWith('image/'))
      if (imageFiles.length > 0) {
        const dt = new DataTransfer()
        imageFiles.forEach((f) => dt.items.add(f))
        onImageAdd(dt.files)
      }
    }
  }, [canAddMore, onImageAdd])

  return { isDragging, handleDragOver, handleDragLeave, handleDrop }
}
