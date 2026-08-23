/**
 * Internationalization setup — vue-i18n bootstrap with locale detection.
 * 国际化初始化 — vue-i18n 引导与语言检测。
 *
 * Supported locales: en, zh-CN. Persists selection in localStorage.
 * 支持语言：en、zh-CN。选择结果持久化在 localStorage 中。
 */
import { createI18n } from 'vue-i18n'

import en from '../locales/en'
import zhCN from '../locales/zh-CN'

export const appLocales = ['en', 'zh-CN'] as const
export type AppLocale = (typeof appLocales)[number]

const STORAGE_KEY = 'pdf-compressor-locale'

function isSupportedLocale(value: string): value is AppLocale {
  return (appLocales as readonly string[]).includes(value)
}

function readStoredLocale(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

function readNavigatorLocale(): string {
  try {
    return window.navigator.languages?.[0] || window.navigator.language
  } catch {
    try {
      return Intl.DateTimeFormat().resolvedOptions().locale
    } catch {
      return 'en'
    }
  }
}

function detectInitialLocale(): AppLocale {
  const savedLocale = readStoredLocale()
  if (savedLocale && isSupportedLocale(savedLocale)) {
    return savedLocale
  }

  const browserLocale = readNavigatorLocale()
  if (browserLocale.toLowerCase().startsWith('zh')) {
    return 'zh-CN'
  }

  return 'en'
}

function applyDocumentLocale(locale: AppLocale) {
  document.documentElement.lang = locale
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

applyDocumentLocale(i18n.global.locale.value)

export function setAppLocale(locale: AppLocale) {
  i18n.global.locale.value = locale
  applyDocumentLocale(locale)
  try {
    window.localStorage.setItem(STORAGE_KEY, locale)
  } catch {
    // Ignore storage failures and keep the in-memory locale.
  }
}
