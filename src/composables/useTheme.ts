/**
 * Theme management composable.
 * 主题管理 composable。
 *
 * Supports dark, light, and system (follows OS preference) modes.
 * 支持深色、浅色和跟随系统（读取操作系统偏好）三种模式。
 *
 * On first launch with no stored preference, defaults to 'system'.
 * 首次启动未存储偏好时，默认为 'system'（跟随系统）。
 *
 * All reactive state and watchers live at module scope so the DOM stays in
 * sync regardless of which components come and go.
 * 所有响应式状态与侦听器都挂在模块作用域，DOM 同步不依赖任何组件的生命周期。
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
const systemDark = ref<boolean>(getSystemPreference() === 'dark')

const resolvedTheme = computed<'dark' | 'light'>(() => {
  if (themePreference.value === 'system') {
    return systemDark.value ? 'dark' : 'light'
  }
  return themePreference.value
})

function applyThemeToDOM(theme: 'dark' | 'light') {
  const root = document.documentElement
  root.setAttribute('data-theme', theme)
  root.style.colorScheme = theme
}

try {
  window
    .matchMedia('(prefers-color-scheme: dark)')
    .addEventListener('change', (event) => {
      systemDark.value = event.matches
    })
} catch {
  // ignore — media query listeners unavailable
}

// Module-scoped watcher: independent of any component's effect scope.
watch(resolvedTheme, (theme) => {
  applyThemeToDOM(theme)
}, { immediate: true })

export function useTheme() {
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
    setTheme,
  }
}
