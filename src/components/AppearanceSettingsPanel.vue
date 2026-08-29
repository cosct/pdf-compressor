<script setup lang="ts">
/**
 * Appearance settings — theme and UI language. Both are global singletons
 * (useTheme's module-scope state, the vue-i18n instance), so this panel
 * talks to them directly instead of round-tripping through App.vue.
 * 外观设置 —— 主题与界面语言。两者都是全局单例（useTheme 的模块级状态、
 * vue-i18n 实例），面板直接读写，不再经 App.vue 中转。
 */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useTheme, type Theme } from '../composables/useTheme'
import { appLocales, setAppLocale, type AppLocale } from '../i18n'

const { t, locale } = useI18n()
const { themePreference, setTheme } = useTheme()

const themeOptions: { value: Theme; labelKey: string }[] = [
  { value: 'system', labelKey: 'theme.system' },
  { value: 'light', labelKey: 'theme.light' },
  { value: 'dark', labelKey: 'theme.dark' },
]

const localeOptions = computed(() =>
  appLocales.map((value) => ({
    value,
    label: t(`locale.${value}`),
  })),
)

function chooseLocale(value: AppLocale) {
  setAppLocale(value)
}
</script>

<template>
  <section class="appearance-panel">
    <header class="panel-header">
      <span class="panel-header__icon" aria-hidden="true">
        <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
          <circle cx="9" cy="9" r="3.2" stroke="currentColor" stroke-width="1.4"/>
          <path d="M9 1.8v1.6M9 14.6v1.6M1.8 9h1.6M14.6 9h1.6M3.9 3.9l1.1 1.1M13 13l1.1 1.1M14.1 3.9 13 5M5 13l-1.1 1.1" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
        </svg>
      </span>
      <h2>{{ t('appearance.title') }}</h2>
    </header>

    <div class="appearance-panel__body">
      <div class="appearance-field">
        <span class="appearance-field__label">{{ t('theme.label') }}</span>
        <div class="appearance-segmented" role="radiogroup" :aria-label="t('theme.label')">
          <button
            v-for="option in themeOptions"
            :key="option.value"
            type="button"
            role="radio"
            :aria-checked="themePreference === option.value ? 'true' : 'false'"
            class="appearance-segmented__option"
            :class="{ 'appearance-segmented__option--active': themePreference === option.value }"
            @click="setTheme(option.value)"
          >
            {{ t(option.labelKey) }}
          </button>
        </div>
      </div>

      <div class="appearance-field">
        <span class="appearance-field__label">{{ t('locale.label') }}</span>
        <div class="appearance-segmented" role="radiogroup" :aria-label="t('locale.label')">
          <button
            v-for="option in localeOptions"
            :key="option.value"
            type="button"
            role="radio"
            :aria-checked="locale === option.value ? 'true' : 'false'"
            class="appearance-segmented__option"
            :class="{ 'appearance-segmented__option--active': locale === option.value }"
            @click="chooseLocale(option.value)"
          >
            {{ option.label }}
          </button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.appearance-panel {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-16);
}

.appearance-panel__body {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-16);
}

.appearance-field {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-8);
  min-width: 0;
}

.appearance-field__label {
  color: var(--fd-text-secondary);
  font: var(--fd-text-body-strong);
}

.appearance-segmented {
  display: flex;
  gap: var(--fd-space-4);
  padding: var(--fd-space-4);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-layer-1);
}

.appearance-segmented__option {
  flex: 1;
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-12);
  border: 1px solid transparent;
  border-radius: calc(var(--fd-radius-sm) - 4px);
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  white-space: nowrap;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard);
}

.appearance-segmented__option:hover {
  background: var(--fd-subtle-bg-hover);
}

.appearance-segmented__option--active {
  background: var(--fd-surface-raised);
  border-color: var(--fd-stroke-card);
  color: var(--fd-text-primary);
  box-shadow: var(--fd-shadow-4);
}

.appearance-segmented__option:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

@media (max-width: 640px) {
  .appearance-panel__body {
    grid-template-columns: 1fr;
  }
}
</style>
