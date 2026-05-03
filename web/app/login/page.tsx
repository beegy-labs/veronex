'use client'

import { useState, useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { api } from '@/lib/api'
import { setSession } from '@/lib/auth'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardHeader, CardTitle, CardDescription, CardFooter } from '@/components/ui/card'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Moon, Sun } from 'lucide-react'
import { useTranslation, i18n } from '@/i18n'
import { locales, localeLabels, localStorageKey, type Locale } from '@/i18n/config'
import { useTheme } from '@/components/theme-provider'

const SAVED_USERNAME_KEY = 'veronex_saved_username'

function readSavedUsername(): string {
  if (typeof document === 'undefined') return ''
  const match = document.cookie.match(/(?:^|;\s*)veronex_saved_username=([^;]*)/)
  return match ? decodeURIComponent(match[1]) : ''
}

function writeSavedUsername(username: string) {
  const expires = new Date(Date.now() + 30 * 864e5).toUTCString()
  document.cookie = `${SAVED_USERNAME_KEY}=${encodeURIComponent(username)}; path=/; expires=${expires}; SameSite=Lax`
}

function clearSavedUsername() {
  document.cookie = `${SAVED_USERNAME_KEY}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Lax`
}

export default function LoginPage() {
  const router = useRouter()
  const { t } = useTranslation()
  const { theme, toggleTheme } = useTheme()

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [rememberUsername, setRememberUsername] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [locale, setLocale] = useState<Locale>('en')

  // On mount: pre-fill saved username + sync locale
  useEffect(() => {
    const saved = readSavedUsername()
    if (saved) {
      setUsername(saved)
      setRememberUsername(true)
    }

    const stored = localStorage.getItem(localStorageKey) as Locale | null
    if (stored && locales.includes(stored)) {
      setLocale(stored)
    } else {
      const browser = navigator.language.slice(0, 2) as Locale
      if (locales.includes(browser)) setLocale(browser)
    }
  }, [])

  function changeLocale(next: Locale) {
    setLocale(next)
    localStorage.setItem(localStorageKey, next)
    i18n.changeLanguage(next)
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)
    try {
      const resp = await api.login({ username, password })
      setSession(resp)
      if (rememberUsername) {
        writeSavedUsername(username)
      } else {
        clearSavedUsername()
      }
      router.push('/')
    } catch {
      setError(t('auth.invalidCredentials'))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="vds-min-h-screen vds-flex vds-items-center vds-justify-center vds-bg-page">
      <Card className="vds-w-full vds-max-w-sm">
        <CardHeader className="vds-p-5 vds-pb-3">
          <CardTitle className="vds-text-xl">{t('auth.login')}</CardTitle>
          <CardDescription>{t('auth.loginDescription')}</CardDescription>
        </CardHeader>
        <CardContent className="vds-p-5 vds-pt-0">
          <form onSubmit={handleSubmit} className="vds-space-y-3">
            <div className="vds-space-y-1">
              <Label htmlFor="username">{t('auth.username')}</Label>
              <Input
                id="username"
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                required
              />
            </div>
            <div className="vds-space-y-1">
              <Label htmlFor="password">{t('auth.password')}</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
                required
              />
            </div>
            <div className="vds-flex vds-items-center vds-gap-2">
              <input
                id="remember"
                type="checkbox"
                checked={rememberUsername}
                onChange={(e) => setRememberUsername(e.target.checked)}
                className="vds-h-4 vds-w-4 vds-rounded vds-border-subtle vds-accent-primary vds-cursor-pointer"
              />
              <Label htmlFor="remember" className="vds-cursor-pointer vds-font-400 vds-text-sm vds-text-dim">
                {t('auth.rememberUsername')}
              </Label>
            </div>
            {error && (
              <p className="vds-text-sm vds-text-destructive">{error}</p>
            )}
            <Button type="submit" className="vds-w-full" disabled={loading}>
              {loading ? t('auth.signingIn') : t('auth.login')}
            </Button>
          </form>
        </CardContent>
        <CardFooter className="vds-flex vds-justify-between vds-items-center vds-p-5 vds-pt-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="vds-h-8 vds-w-8"
            onClick={toggleTheme}
            aria-label={theme === 'dark' ? t('common.switchToLight') : t('common.switchToDark')}
          >
            {theme === 'dark' ? <Sun className="vds-h-4 vds-w-4" /> : <Moon className="vds-h-4 vds-w-4" />}
          </Button>
          <Select value={locale} onValueChange={(v) => changeLocale(v as Locale)}>
            <SelectTrigger className="vds-w-32 vds-h-8 vds-text-xs" aria-label={t('common.language')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {locales.map((l) => (
                <SelectItem key={l} value={l} className="vds-text-xs">
                  {localeLabels[l]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardFooter>
      </Card>
    </div>
  )
}
