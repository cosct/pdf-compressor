<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { AppLocale } from '../i18n'
import { useTheme, type Theme } from '../composables/useTheme'
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
  locale: AppLocale
  locales: readonly AppLocale[]
}>()

const emit = defineEmits<{
  'update:locale': [value: AppLocale]
}>()

const { t } = useI18n()
const { themePreference, setTheme } = useTheme()
const windowMaximized = ref(false)
const openPicker = ref<'theme' | 'locale' | null>(null)
const headerRoot = ref<HTMLElement | null>(null)
const themeTriggerRef = ref<HTMLButtonElement | null>(null)
const localeTriggerRef = ref<HTMLButtonElement | null>(null)
const themeMenuRef = ref<HTMLElement | null>(null)
const localeMenuRef = ref<HTMLElement | null>(null)
let lastTitlebarMouseDownAt = 0

const activeMenuRef = computed(() => {
  if (openPicker.value === 'theme') {
    return themeMenuRef.value
  }
  return openPicker.value === 'locale' ? localeMenuRef.value : null
})

const activeTriggerRef = computed(() => {
  if (openPicker.value === 'theme') {
    return themeTriggerRef.value
  }
  return openPicker.value === 'locale' ? localeTriggerRef.value : null
})

const localeOptions = computed(() =>
  props.locales.map((locale) => ({
    value: locale,
    label: t(`locale.${locale}`),
  })),
)

const themeOptions: { value: Theme; labelKey: string }[] = [
  { value: 'system', labelKey: 'theme.system' },
  { value: 'light', labelKey: 'theme.light' },
  { value: 'dark', labelKey: 'theme.dark' },
]

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

function togglePicker(name: 'theme' | 'locale') {
  if (openPicker.value === name) {
    closePickers()
    return
  }

  openPicker.value = name
  void nextTick(() => {
    // WAI-ARIA menu pattern: move focus into the menu on open.
    activeMenuRef.value
      ?.querySelector<HTMLElement>('[role="menuitemradio"]')
      ?.focus()
  })
}

function closePickers() {
  // If focus sits inside the menu, hand it back to the trigger button.
  const menuEl = activeMenuRef.value
  if (menuEl && document.activeElement && menuEl.contains(document.activeElement)) {
    activeTriggerRef.value?.focus()
  }

  openPicker.value = null
}

function handleMenuKeydown(event: KeyboardEvent) {
  const items = Array.from(
    activeMenuRef.value?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? [],
  )
  if (!items.length) {
    return
  }

  const currentIndex = items.indexOf(document.activeElement as HTMLElement)
  let nextIndex: number | null = null

  switch (event.key) {
    case 'ArrowDown':
      nextIndex = (currentIndex + 1 + items.length) % items.length
      break
    case 'ArrowUp':
      nextIndex = (currentIndex - 1 + items.length) % items.length
      break
    case 'Home':
      nextIndex = 0
      break
    case 'End':
      nextIndex = items.length - 1
      break
    case 'Tab':
      closePickers()
      return
    default:
      return
  }

  event.preventDefault()
  items[nextIndex]?.focus()
}

function chooseTheme(value: Theme) {
  setTheme(value)
  closePickers()
}

function chooseLocale(value: AppLocale) {
  emit('update:locale', value)
  closePickers()
}

function handleDocumentPointerDown(event: PointerEvent) {
  if (!headerRoot.value?.contains(event.target as Node)) {
    closePickers()
  }
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closePickers()
  }
}

onMounted(async () => {
  void syncWindowState()
  document.addEventListener('pointerdown', handleDocumentPointerDown)
  document.addEventListener('keydown', handleDocumentKeydown)

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
  document.removeEventListener('pointerdown', handleDocumentPointerDown)
  document.removeEventListener('keydown', handleDocumentKeydown)
  unlistenResize?.()
  unlistenResize = null
})
</script>

<template>
  <header ref="headerRoot" class="app-header">
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
        <div class="titlebar__prefs">
          <div class="picker picker--theme" :class="{ 'picker--open': openPicker === 'theme' }">
            <button
              ref="themeTriggerRef"
              class="picker__trigger"
              type="button"
              :aria-expanded="openPicker === 'theme' ? 'true' : 'false'"
              :aria-label="t('theme.label')"
              @click.stop="togglePicker('theme')"
            >
              <span class="picker__icon" aria-hidden="true">
                <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                  <path d="M6 1.5v1.2M6 9.3v1.2M2.82 2.82l.85.85M8.33 8.33l.85.85M1.5 6h1.2M9.3 6h1.2M2.82 9.18l.85-.85M8.33 3.67l.85-.85" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>
                  <circle cx="6" cy="6" r="2.15" stroke="currentColor" stroke-width="1.1"/>
                </svg>
              </span>
              <span class="picker__label">{{ t('theme.label') }}</span>
            </button>

            <transition name="picker-menu">
              <div
                v-if="openPicker === 'theme'"
                ref="themeMenuRef"
                class="picker__menu"
                role="menu"
                :aria-label="t('theme.label')"
                @keydown="handleMenuKeydown"
              >
                <button
                  v-for="option in themeOptions"
                  :key="option.value"
                  class="picker__option"
                  :class="{ 'picker__option--active': themePreference === option.value }"
                  type="button"
                  role="menuitemradio"
                  :aria-checked="themePreference === option.value ? 'true' : 'false'"
                  @click="chooseTheme(option.value)"
                >
                  <span>{{ t(option.labelKey) }}</span>
                  <svg
                    v-if="themePreference === option.value"
                    class="picker__check"
                    width="12"
                    height="12"
                    viewBox="0 0 12 12"
                    fill="none"
                    aria-hidden="true"
                  >
                    <path d="M2.5 6.2 4.9 8.5 9.5 3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
                  </svg>
                </button>
              </div>
            </transition>
          </div>

          <div class="picker picker--locale" :class="{ 'picker--open': openPicker === 'locale' }">
            <button
              ref="localeTriggerRef"
              class="picker__trigger"
              type="button"
              :aria-expanded="openPicker === 'locale' ? 'true' : 'false'"
              :aria-label="t('locale.label')"
              @click.stop="togglePicker('locale')"
            >
              <span class="picker__icon" aria-hidden="true">
                <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                  <path d="M2.2 3.2h5.6M5 1.8c0 3-.95 5.23-2.85 6.7M3.8 5.6c.72 1.18 1.72 2.16 3 2.95M8.45 2.05h1.95l1.15 3.15H7.3l1.15-3.15Z" stroke="currentColor" stroke-width="1.05" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
              </span>
              <span class="picker__label">{{ t('locale.label') }}</span>
            </button>

            <transition name="picker-menu">
              <div
                v-if="openPicker === 'locale'"
                ref="localeMenuRef"
                class="picker__menu"
                role="menu"
                :aria-label="t('locale.label')"
                @keydown="handleMenuKeydown"
              >
                <button
                  v-for="option in localeOptions"
                  :key="option.value"
                  class="picker__option"
                  :class="{ 'picker__option--active': props.locale === option.value }"
                  type="button"
                  role="menuitemradio"
                  :aria-checked="props.locale === option.value ? 'true' : 'false'"
                  @click="chooseLocale(option.value)"
                >
                  <span>{{ option.label }}</span>
                  <svg
                    v-if="props.locale === option.value"
                    class="picker__check"
                    width="12"
                    height="12"
                    viewBox="0 0 12 12"
                    fill="none"
                    aria-hidden="true"
                  >
                    <path d="M2.5 6.2 4.9 8.5 9.5 3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
                  </svg>
                </button>
              </div>
            </transition>
          </div>
        </div>

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

.titlebar__controls {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  min-height: 40px;
}

.titlebar__prefs {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.picker {
  position: relative;
  min-width: 0;
}

.picker--theme {
  min-width: 88px;
}

.picker--locale {
  min-width: 88px;
}

.picker__trigger {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: var(--fd-space-6);
  min-height: var(--fd-control-height-sm);
  min-width: 0;
  white-space: nowrap;
  padding: 0 12px 0 9px;
  border: 1px solid var(--fd-control-stroke);
  border-radius: 999px;
  background: var(--fd-control-bg);
  color: var(--fd-text-primary);
  cursor: pointer;
  transition:
    border-color var(--fd-duration-fast) var(--fd-easing-standard),
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    box-shadow var(--fd-duration-fast) var(--fd-easing-standard);
}

.picker__trigger:hover {
  background: var(--fd-control-bg-hover);
}

.picker__trigger:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.picker--open .picker__trigger {
  border-color: var(--fd-accent);
  box-shadow: var(--fd-shadow-focus);
}

.picker__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  border-radius: 999px;
  color: var(--fd-text-secondary);
  background: var(--fd-layer-1);
  border: 1px solid var(--fd-stroke-soft);
}

.picker__label {
  white-space: nowrap;
  font: var(--fd-text-caption);
  font-weight: 600;
}

.picker__menu {
  position: absolute;
  top: calc(100% + 8px);
  left: 0;
  min-width: 124px;
  z-index: 30;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 16px;
  background: var(--fd-flyout-bg);
  box-shadow: var(--fd-shadow-8);
  backdrop-filter: blur(24px) saturate(150%);
  -webkit-backdrop-filter: blur(24px) saturate(150%);
}

.picker__option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-10);
  min-height: var(--fd-control-height-md);
  padding: 0 10px;
  border: none;
  border-radius: 12px;
  background: transparent;
  color: var(--fd-text-primary);
  cursor: pointer;
  text-align: left;
  white-space: nowrap;
}

.picker__option:hover,
.picker__option--active {
  background: var(--fd-subtle-bg-hover);
}

.picker__option--active {
  color: var(--fd-accent);
}

.picker__check {
  color: inherit;
  flex-shrink: 0;
}

.picker-menu-enter-active,
.picker-menu-leave-active {
  transition:
    opacity var(--fd-duration-normal) var(--fd-easing-standard),
    transform var(--fd-duration-normal) var(--fd-easing-standard);
}

.picker-menu-enter-from,
.picker-menu-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

@media (max-width: 980px) {
  .titlebar {
    grid-template-columns: 1fr;
    padding: var(--fd-space-8) 0;
    gap: var(--fd-space-8);
  }

  .titlebar__controls,
  .titlebar__prefs {
    width: 100%;
  }

  .picker {
    flex: 1;
    min-width: 0;
  }

  .picker__trigger {
    width: 100%;
  }

  .window-controls {
    margin-left: 0;
    border-left: none;
  }
}

@media (max-width: 768px) {
  .app-header {
    padding: 0 var(--fd-space-12);
  }

  .titlebar__controls,
  .titlebar__prefs {
    flex-direction: column;
  }
}
</style>
