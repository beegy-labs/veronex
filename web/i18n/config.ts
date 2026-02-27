export const locales = ['en', 'ko', 'ja'] as const
export type Locale = (typeof locales)[number]
export const defaultLocale: Locale = 'en'

export const localeLabels: Record<Locale, string> = {
  en: 'EN',
  ko: '한국어',
  ja: '日本語',
}

export const localStorageKey = 'hg-lang'
