'use client'

import { useState, useCallback } from 'react'
import { useMutation } from '@tanstack/react-query'
import { verifyErrorMessage } from '@/lib/api'
import type { VerifyState } from '@/lib/types'

interface VerifyLabels {
  duplicate: string; network: string; unreachable: string; fallback: string
}

interface UseVerifyUrlOptions {
  verifyFn: (url: string) => Promise<{ reachable: boolean }>
  labels: VerifyLabels
  initialUrl?: string
}

export function useVerifyUrl({ verifyFn, labels, initialUrl = '' }: UseVerifyUrlOptions) {
  const [verifyState, setVerifyState] = useState<VerifyState>('idle')
  const [verifyError, setVerifyError] = useState('')
  const [verifiedUrl, setVerifiedUrl] = useState(initialUrl)

  const mutation = useMutation({
    mutationFn: (url: string) => verifyFn(url),
    onSuccess: (_data, url) => { setVerifyState('ok'); setVerifiedUrl(url) },
    onError: (e) => {
      setVerifyState('error')
      setVerifyError(verifyErrorMessage(e, labels))
    },
  })

  const handleUrlChange = useCallback(() => {
    if (verifyState !== 'idle') { setVerifyState('idle'); setVerifyError('') }
  }, [verifyState])

  const verify = useCallback((url: string) => {
    setVerifyState('checking')
    mutation.mutate(url)
  }, [mutation])

  return { verifyState, verifyError, verifiedUrl, verify, handleUrlChange }
}
