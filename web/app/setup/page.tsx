'use client'

import { useEffect, useState } from 'react'
import { useRouter } from 'next/navigation'
import { api } from '@/lib/api'
import { setSession } from '@/lib/auth'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from '@/components/ui/card'
import { useTranslation } from '@/i18n'

type Step = 'loading' | 'account' | 'storage' | 'restart' | 'done'

export default function SetupPage() {
  const { t } = useTranslation()
  const router = useRouter()
  const [step, setStep] = useState<Step>('loading')

  // Step 1 — account
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')

  // Step 2 — storage
  const [s3Endpoint, setS3Endpoint] = useState('http://minio:9000')
  const [s3Region, setS3Region] = useState('us-east-1')
  const [s3AccessKey, setS3AccessKey] = useState('')
  const [s3SecretKey, setS3SecretKey] = useState('')
  const [s3ModelBucket, setS3ModelBucket] = useState('veronex-models')
  const [hfToken, setHfToken] = useState('')
  const [modelLocalPath, setModelLocalPath] = useState('/var/lib/veronex/models')
  const [modelMaxDiskGb, setModelMaxDiskGb] = useState('100')

  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  // Decide which step to show based on the v2 status response. Account
  // first; once an account exists we proceed to storage; once both are
  // done, redirect home.
  useEffect(() => {
    let alive = true
    api
      .setupStatus()
      .then((s) => {
        if (!alive) return
        if (s.setup_complete) {
          router.replace('/')
          return
        }
        if (s.needs_setup_account) {
          setStep('account')
        } else if (s.needs_setup_storage) {
          setStep('storage')
        } else {
          setStep('done')
        }
      })
      .catch(() => {
        if (!alive) return
        // If status itself errors, fall back to account step — operator
        // can still drive the wizard by hand.
        setStep('account')
      })
    return () => {
      alive = false
    }
  }, [router])

  async function submitAccount(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    if (password.length < 8) {
      setError(t('setup.passwordTooShort'))
      return
    }
    if (password !== confirm) {
      setError(t('setup.passwordMismatch'))
      return
    }
    setLoading(true)
    try {
      const resp = await api.setup({ username: username.trim(), password })
      setSession(resp)
      // Account created → check whether storage still needs setup.
      const status = await api.setupStatus()
      if (status.needs_setup_storage) {
        setStep('storage')
      } else {
        router.push('/')
      }
    } catch (err: unknown) {
      if (err instanceof Error && err.message.startsWith('409')) {
        setError(t('setup.alreadySetup'))
      } else {
        setError(t('setup.failed'))
      }
    } finally {
      setLoading(false)
    }
  }

  async function submitStorage(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    if (
      !s3Endpoint.trim() ||
      !s3AccessKey.trim() ||
      !s3SecretKey ||
      !s3ModelBucket.trim()
    ) {
      setError(t('setup.storage.missingRequired'))
      return
    }
    const maxGb = modelMaxDiskGb.trim() === '' ? null : Number(modelMaxDiskGb)
    if (maxGb !== null && (Number.isNaN(maxGb) || maxGb <= 0)) {
      setError(t('setup.storage.invalidMaxDisk'))
      return
    }
    setLoading(true)
    try {
      await api.setupStorage({
        s3_endpoint: s3Endpoint.trim(),
        s3_region: s3Region.trim() || null,
        s3_access_key: s3AccessKey.trim(),
        s3_secret_key: s3SecretKey,
        s3_model_bucket: s3ModelBucket.trim(),
        hf_token: hfToken.trim() || null,
        model_local_path: modelLocalPath.trim() || null,
        model_max_disk_gb: maxGb,
      })
      setStep('restart')
    } catch (err: unknown) {
      setError(
        err instanceof Error ? err.message : t('setup.storage.failed'),
      )
    } finally {
      setLoading(false)
    }
  }

  if (step === 'loading') {
    return (
      <div className="vds-min-h-screen vds-flex vds-items-center vds-justify-center vds-bg-page">
        <p className="vds-text-sm vds-text-muted-foreground">{t('setup.checking')}</p>
      </div>
    )
  }

  if (step === 'restart') {
    return (
      <div className="vds-min-h-screen vds-flex vds-items-center vds-justify-center vds-bg-page">
        <Card className="vds-w-full vds-max-w-md">
          <CardHeader>
            <CardTitle className="vds-text-xl">{t('setup.restart.title')}</CardTitle>
            <CardDescription>{t('setup.restart.description')}</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="vds-text-sm">{t('setup.restart.body')}</p>
          </CardContent>
        </Card>
      </div>
    )
  }

  if (step === 'storage') {
    return (
      <div className="vds-min-h-screen vds-flex vds-items-center vds-justify-center vds-bg-page vds-py-8">
        <Card className="vds-w-full vds-max-w-lg">
          <CardHeader>
            <CardTitle className="vds-text-xl">{t('setup.storage.title')}</CardTitle>
            <CardDescription>{t('setup.storage.description')}</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={submitStorage} className="vds-space-y-4">
              <div className="vds-space-y-1.5">
                <Label htmlFor="s3-endpoint">{t('setup.storage.endpoint')}</Label>
                <Input
                  id="s3-endpoint"
                  type="url"
                  value={s3Endpoint}
                  onChange={(e) => setS3Endpoint(e.target.value)}
                  required
                />
              </div>
              <div className="vds-grid vds-grid-cols-2 vds-gap-3">
                <div className="vds-space-y-1.5">
                  <Label htmlFor="s3-region">{t('setup.storage.region')}</Label>
                  <Input
                    id="s3-region"
                    type="text"
                    value={s3Region}
                    onChange={(e) => setS3Region(e.target.value)}
                  />
                </div>
                <div className="vds-space-y-1.5">
                  <Label htmlFor="s3-bucket">{t('setup.storage.bucket')}</Label>
                  <Input
                    id="s3-bucket"
                    type="text"
                    value={s3ModelBucket}
                    onChange={(e) => setS3ModelBucket(e.target.value)}
                    required
                  />
                </div>
              </div>
              <div className="vds-grid vds-grid-cols-2 vds-gap-3">
                <div className="vds-space-y-1.5">
                  <Label htmlFor="s3-access">{t('setup.storage.accessKey')}</Label>
                  <Input
                    id="s3-access"
                    type="text"
                    value={s3AccessKey}
                    onChange={(e) => setS3AccessKey(e.target.value)}
                    autoComplete="off"
                    required
                  />
                </div>
                <div className="vds-space-y-1.5">
                  <Label htmlFor="s3-secret">{t('setup.storage.secretKey')}</Label>
                  <Input
                    id="s3-secret"
                    type="password"
                    value={s3SecretKey}
                    onChange={(e) => setS3SecretKey(e.target.value)}
                    autoComplete="off"
                    required
                  />
                </div>
              </div>
              <div className="vds-space-y-1.5">
                <Label htmlFor="hf-token">{t('setup.storage.hfToken')}</Label>
                <Input
                  id="hf-token"
                  type="password"
                  value={hfToken}
                  onChange={(e) => setHfToken(e.target.value)}
                  autoComplete="off"
                  placeholder={t('setup.storage.hfTokenOptional')}
                />
              </div>
              <div className="vds-grid vds-grid-cols-2 vds-gap-3">
                <div className="vds-space-y-1.5">
                  <Label htmlFor="model-path">{t('setup.storage.localPath')}</Label>
                  <Input
                    id="model-path"
                    type="text"
                    value={modelLocalPath}
                    onChange={(e) => setModelLocalPath(e.target.value)}
                  />
                </div>
                <div className="vds-space-y-1.5">
                  <Label htmlFor="max-disk">{t('setup.storage.maxDiskGb')}</Label>
                  <Input
                    id="max-disk"
                    type="number"
                    min={1}
                    value={modelMaxDiskGb}
                    onChange={(e) => setModelMaxDiskGb(e.target.value)}
                  />
                </div>
              </div>
              {error && (
                <p className="vds-text-sm vds-text-destructive">{error}</p>
              )}
              <Button type="submit" className="vds-w-full" disabled={loading}>
                {loading ? t('setup.submitting') : t('setup.storage.submit')}
              </Button>
            </form>
          </CardContent>
        </Card>
      </div>
    )
  }

  // step === 'account' or 'done'
  return (
    <div className="vds-min-h-screen vds-flex vds-items-center vds-justify-center vds-bg-page">
      <Card className="vds-w-full vds-max-w-sm">
        <CardHeader>
          <CardTitle className="vds-text-xl">{t('setup.title')}</CardTitle>
          <CardDescription>{t('setup.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submitAccount} className="vds-space-y-4">
            <div className="vds-space-y-1.5">
              <Label htmlFor="username">{t('setup.username')}</Label>
              <Input
                id="username"
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                required
              />
            </div>
            <div className="vds-space-y-1.5">
              <Label htmlFor="password">{t('setup.password')}</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="new-password"
                minLength={8}
                required
              />
            </div>
            <div className="vds-space-y-1.5">
              <Label htmlFor="confirm">{t('setup.confirmPassword')}</Label>
              <Input
                id="confirm"
                type="password"
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                autoComplete="new-password"
                required
              />
            </div>
            {error && (
              <p className="vds-text-sm vds-text-destructive">{error}</p>
            )}
            <Button type="submit" className="vds-w-full" disabled={loading}>
              {loading ? t('setup.submitting') : t('setup.submit')}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
