'use client'

import { useState, useEffect } from 'react'
import { i18n } from '@/i18n'
import { locales, localeLabels, localStorageKey, type Locale } from '@/i18n/config'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

export function LanguageSwitcher() {
  const [current, setCurrent] = useState<Locale>('en')

  useEffect(() => {
    const stored = localStorage.getItem(localStorageKey) as Locale | null
    if (stored && locales.includes(stored)) setCurrent(stored)
    else setCurrent((i18n.language?.slice(0, 2) as Locale) ?? 'en')
  }, [])

  function changeLocale(locale: Locale) {
    setCurrent(locale)
    localStorage.setItem(localStorageKey, locale)
    i18n.changeLanguage(locale)
  }

  return (
    <Select value={current} onValueChange={(v) => changeLocale(v as Locale)}>
      <SelectTrigger className="h-7 text-xs border-0 bg-transparent px-1 text-muted-foreground focus:ring-0 w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="start" side="top">
        {locales.map((locale) => (
          <SelectItem key={locale} value={locale} className="text-xs">
            {localeLabels[locale]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
