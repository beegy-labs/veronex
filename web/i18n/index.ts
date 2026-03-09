import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import { defaultLocale, localStorageKey, type Locale, locales } from './config'

import en from '../messages/en.json'
import ko from '../messages/ko.json'
import ja from '../messages/ja.json'

export function detectLocale(): Locale {
  if (typeof window === 'undefined') return defaultLocale
  const stored = localStorage.getItem(localStorageKey)
  if (stored && locales.includes(stored as Locale)) return stored as Locale
  const browser = navigator.language.slice(0, 2)
  if (locales.includes(browser as Locale)) return browser as Locale
  return defaultLocale
}

if (!i18n.isInitialized) {
  i18n.use(initReactI18next).init({
    // Always start with defaultLocale to match server render (avoid hydration mismatch).
    // Client-side locale detection happens in I18nProvider via useEffect.
    lng: defaultLocale,
    fallbackLng: defaultLocale,
    resources: {
      en: { translation: en },
      ko: { translation: ko },
      ja: { translation: ja },
    },
    interpolation: { escapeValue: false },
    // Suppress Locize sponsorship console.info
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ...({ showSupportNotice: false } as any),
  })
}

export { i18n }
export { useTranslation } from 'react-i18next'
