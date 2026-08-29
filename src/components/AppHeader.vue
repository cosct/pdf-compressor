<script setup lang="ts">
/**
 * Application header — frameless titlebar with brand, settings navigation,
 * and window controls. Theme and language selection live in the settings
 * view (AppearanceSettingsPanel) now.
 * 应用头部 —— 无边框标题栏，含品牌、设置导航与窗口控制。
 * 主题与语言选择已移入设置视图（AppearanceSettingsPanel）。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  closeAppWindow,
  isAppWindowMaximized,
  minimizeAppWindow,
  startDraggingAppWindow,
  toggleAppWindowMaximize,
} from '../lib/tauri'

let unlistenResize: (() => void) | null = null

const props = defineProps<{
  nativeAvailable: boolean
  /** Which top-level view is showing — drives the settings/back button. */
  view: 'main' | 'settings'
}>()

const emit = defineEmits<{
  'toggle-settings': []
}>()

const { t } = useI18n()
const windowMaximized = ref(false)
let lastTitlebarMouseDownAt = 0

async function syncWindowState() {
  if (!props.nativeAvailable) {
    windowMaximized.value = false
    return
  }

  windowMaximized.value = await isAppWindowMaximized()
}

async function handleMinimizeClick() {
  try {
    await minimizeAppWindow()
  } catch (error) {
    console.warn('Failed to minimize window:', error)
  }
}

async function handleToggleWindowState() {
  try {
    windowMaximized.value = await toggleAppWindowMaximize()
  } catch (error) {
    console.warn('Failed to toggle window state:', error)
  }
}

async function handleCloseClick() {
  try {
    await closeAppWindow()
  } catch (error) {
    console.warn('Failed to close window:', error)
  }
}

function handleTitlebarDoubleClick() {
  if (!props.nativeAvailable) {
    return
  }

  void handleToggleWindowState()
}

function handleTitlebarMouseDown(event: MouseEvent) {
  if (!props.nativeAvailable || event.button !== 0) {
    return
  }

  // Second press of a double-click: skip dragging so the dblclick handler
  // can maximize without fighting a native drag loop.
  const now = Date.now()
  if (now - lastTitlebarMouseDownAt < 400) {
    lastTitlebarMouseDownAt = 0
    return
  }
  lastTitlebarMouseDownAt = now

  void startDraggingAppWindow()
}

onMounted(async () => {
  void syncWindowState()

  if (props.nativeAvailable) {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      unlistenResize = await getCurrentWindow().onResized(() => {
        void syncWindowState()
      })
    } catch {
      // ignore — window events unavailable
    }
  }
})

onBeforeUnmount(() => {
  unlistenResize?.()
  unlistenResize = null
})
</script>

<template>
  <header class="app-header">
    <div class="titlebar">
      <div class="titlebar__drag" @mousedown="handleTitlebarMouseDown" @dblclick="handleTitlebarDoubleClick">
        <div class="titlebar__brand">
          <div class="titlebar__icon" aria-hidden="true">
            <svg width="20" height="20" viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
              <rect x="5" y="4" width="22" height="24" rx="6" stroke="currentColor" stroke-width="1.6"/>
              <path d="M10 11.5h12M10 16h12M10 20.5h7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
              <path d="M21 18.5l3 3-3 3" stroke="var(--fd-accent)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
              <path d="M24 21.5h-8" stroke="var(--fd-accent)" stroke-width="1.8" stroke-linecap="round"/>
            </svg>
          </div>
          <strong>{{ t('header.title') }}</strong>
        </div>
      </div>

      <div class="titlebar__controls">
        <button
          class="nav-button"
          type="button"
          :aria-label="props.view === 'settings' ? t('header.back') : t('header.settings')"
          :title="props.view === 'settings' ? t('header.back') : t('header.settings')"
          @click.stop="emit('toggle-settings')"
        >
          <span class="nav-button__icon" aria-hidden="true">
            <svg v-if="props.view === 'settings'" width="13" height="13" viewBox="0 0 12 12" fill="none">
              <path d="M7.5 2.5 4 6l3.5 3.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            <svg v-else width="13" height="13" viewBox="0 0 13 13" fill="none">
              <circle cx="6.5" cy="6.5" r="1.9" stroke="currentColor" stroke-width="1.15"/>
              <path d="M6.5 1.2v1.5M6.5 10.3v1.5M1.2 6.5h1.5M10.3 6.5h1.5M2.75 2.75l1.06 1.06M9.19 9.19l1.06 1.06M10.25 2.75 9.19 3.81M3.81 9.19 2.75 10.25" stroke="currentColor" stroke-width="1.15" stroke-linecap="round"/>
            </svg>
          </span>
          <span class="nav-button__label">{{ props.view === 'settings' ? t('header.back') : t('header.settings') }}</span>
        </button>

        <div v-if="props.nativeAvailable" class="window-controls">
          <button
            class="window-control"
            type="button"
            :title="t('header.minimize')"
            :aria-label="t('header.minimize')"
            @click.stop="handleMinimizeClick"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <path d="M2 6h8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            </svg>
          </button>

          <button
            class="window-control"
            type="button"
            :title="windowMaximized ? t('header.restore') : t('header.maximize')"
            :aria-label="windowMaximized ? t('header.restore') : t('header.maximize')"
            @click.stop="handleToggleWindowState"
          >
            <svg v-if="!windowMaximized" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <rect x="2.5" y="2.5" width="7" height="7" rx="1" stroke="currentColor" stroke-width="1.2"/>
            </svg>
            <svg v-else width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <path d="M3.5 2.5h5a1 1 0 0 1 1 1v5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
              <rect x="2.5" y="4.5" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.2"/>
            </svg>
          </button>

          <button
            class="window-control window-control--danger"
            type="button"
            :title="t('header.close')"
            :aria-label="t('header.close')"
            @click.stop="handleCloseClick"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <path d="M3 3l6 6M9 3 3 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            </svg>
          </button>
        </div>
      </div>
    </div>
  </header>
</template>

<style scoped>
.app-header {
  position: relative;
  z-index: 20;
  padding: 0 var(--fd-space-20);
  border-bottom: 1px solid var(--fd-stroke-card);
  background: var(--fd-bg-acrylic);
  backdrop-filter: blur(16px) saturate(120%);
  -webkit-backdrop-filter: blur(16px) saturate(120%);
}

.titlebar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  width: 100%;
  min-height: 40px;
  gap: var(--fd-space-10);
}

.titlebar__drag {
  display: flex;
  align-items: center;
  min-width: 0;
  min-height: 40px;
  padding: 0;
  user-select: none;
}

.titlebar__brand {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  color: var(--fd-text-secondary);
}

.titlebar__brand strong {
  font: var(--fd-text-body-strong);
  white-space: nowrap;
}

.titlebar__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 8px;
  color: var(--fd-text-primary);
  background: var(--fd-control-bg);
  border: 1px solid var(--fd-control-stroke);
}

.titlebar__controls {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  min-height: 40px;
}

.nav-button {
  display: flex;
  align-items: center;
  gap: var(--fd-space-6);
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-12);
  border: 1px solid var(--fd-control-stroke);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-control-bg);
  color: var(--fd-text-secondary);
  cursor: pointer;
  font: var(--fd-text-caption);
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard),
    border-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.nav-button:hover {
  background: var(--fd-control-bg-hover);
  color: var(--fd-text-primary);
}

.nav-button:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.nav-button__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.window-controls {
  display: flex;
  align-items: stretch;
  margin-left: var(--fd-space-8);
  border-left: 1px solid var(--fd-stroke-soft);
}

.window-control {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: 0;
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard);
}

.window-control:hover {
  background: var(--fd-subtle-bg-hover);
  color: var(--fd-text-primary);
}

.window-control--danger:hover {
  background: var(--fd-danger-strong);
  color: #fff;
}
</style>
