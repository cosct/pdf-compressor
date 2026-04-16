/**
 * Theme management composable.
 * 主题管理 composable。
 *
 * Supports dark, light, and system (follows OS preference) modes.
 * 支持深色、浅色和跟随系统（读取操作系统偏好）三种模式。
 *
 * On first launch with no stored preference, defaults to 'system'.
 * 首次启动未存储偏好时，默认为 'system'（跟随系统）。
 */
import { computed, ref, watch } from 'vue'

export type Theme = 'dark' | 'light' | 'system'

const STORAGE_KEY = 'pdf-compressor-theme'

function getSystemPreference(): 'dark' | 'light' {
  try {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  } catch {
    return 'dark'
  }
}

function readStoredTheme(): Theme | null {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    if (stored === 'dark' || stored === 'light' || stored === 'system') {
      return stored
    }
  } catch {
    // ignore
  }
  return null
}

const themePreference = ref<Theme>(readStoredTheme() ?? 'system')

const resolvedTheme = computed<'dark' | 'light'>(() => {
  if (themePreference.value === 'system') {
    return getSystemPreference()
  }
  return themePreference.value
})

function applyThemeToDOM(theme: 'dark' | 'light') {
  const root = document.documentElement
  root.setAttribute('data-theme', theme)
  root.style.colorScheme = theme
}

let initialized = false

function ensureInitialized() {
  if (initialized) {
    return
  }
  initialized = true

  try {
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      if (themePreference.value === 'system') {
        applyThemeToDOM(getSystemPreference())
      }
    })
  } catch {
    // ignore
  }

  watch(resolvedTheme, (theme) => {
    applyThemeToDOM(theme)
  }, { immediate: true })
}

export function useTheme() {
  ensureInitialized()

  function setTheme(theme: Theme) {
    themePreference.value = theme
    try {
      window.localStorage.setItem(STORAGE_KEY, theme)
    } catch {
      // ignore
    }
  }

  return {
    themePreference,
    resolvedTheme,
    setTheme,
  }
}
