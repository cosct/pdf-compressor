import { createI18n } from 'vue-i18n'

import en from '../locales/en'
import zhCN from '../locales/zh-CN'

export const appLocales = ['en', 'zh-CN'] as const
export type AppLocale = (typeof appLocales)[number]

const STORAGE_KEY = 'pdf-compressor-locale'

function isSupportedLocale(value: string): value is AppLocale {
  return (appLocales as readonly string[]).includes(value)
}

function detectInitialLocale(): AppLocale {
  const savedLocale = window.localStorage.getItem(STORAGE_KEY)
  if (savedLocale && isSupportedLocale(savedLocale)) {
    return savedLocale
  }

  const browserLocale = window.navigator.language
  if (browserLocale.toLowerCase().startsWith('zh')) {
    return 'zh-CN'
  }

  return 'en'
}

export const i18n = createI18n({
  legacy: false,
  locale: detectInitialLocale(),
  fallbackLocale: 'en',
  messages: {
    en,
    'zh-CN': zhCN,
  },
})

export function setAppLocale(locale: AppLocale) {
  i18n.global.locale.value = locale
  window.localStorage.setItem(STORAGE_KEY, locale)
}
