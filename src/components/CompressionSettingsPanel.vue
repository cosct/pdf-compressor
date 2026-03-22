<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { CompressionPreset, CompressionSettings } from '../types/pdf'

const props = defineProps<{
  settings: CompressionSettings
  disabled: boolean
  recommendedPreset: CompressionPreset | null | undefined
}>()

const emit = defineEmits<{
  'update:settings': [value: CompressionSettings]
}>()

const { t } = useI18n()

const presetOptions = computed<
  Array<{ value: CompressionPreset; title: string; description: string }>
>(() => [
  {
    value: 'conservative',
    title: t('app.preset.conservative'),
    description: t('settings.presetDescriptions.conservative'),
  },
  {
    value: 'balanced',
    title: t('app.preset.balanced'),
    description: t('settings.presetDescriptions.balanced'),
  },
  {
    value: 'maximum',
    title: t('app.preset.maximum'),
    description: t('settings.presetDescriptions.maximum'),
  },
])

const settingsMetrics = computed(() => [
  {
    label: t('app.profile'),
    value: t(`app.preset.${props.settings.preset}`),
  },
  {
    label: t('settings.quality'),
    value: String(props.settings.imageQuality),
  },
  {
    label: t('settings.maxEdge'),
    value: `${props.settings.maxImageSizePx} px`,
  },
])

function updateSetting<K extends keyof CompressionSettings>(key: K, value: CompressionSettings[K]) {
  emit('update:settings', {
    ...props.settings,
    [key]: value,
  })
}

function handlePresetKeydown(event: KeyboardEvent, index: number) {
  if (props.disabled) {
    return
  }

  let nextIndex = index

  switch (event.key) {
    case 'ArrowRight':
    case 'ArrowDown':
      nextIndex = (index + 1) % presetOptions.value.length
      break
    case 'ArrowLeft':
    case 'ArrowUp':
      nextIndex = (index - 1 + presetOptions.value.length) % presetOptions.value.length
      break
    case 'Home':
      nextIndex = 0
      break
    case 'End':
      nextIndex = presetOptions.value.length - 1
      break
    default:
      return
  }

  event.preventDefault()
  updateSetting('preset', presetOptions.value[nextIndex].value)

  const currentTarget = event.currentTarget
  if (!(currentTarget instanceof HTMLElement)) {
    return
  }

  const nextPresetButton = currentTarget.parentElement?.querySelector<HTMLButtonElement>(
    `[data-preset-index="${nextIndex}"]`,
  )
  nextPresetButton?.focus()
}
</script>

<template>
  <section class="panel-surface settings-panel">
    <div class="section-header settings-panel__header">
      <div>
        <p class="section-kicker">{{ t('settings.eyebrow') }}</p>
        <h2>{{ t('settings.title') }}</h2>
        <p class="settings-panel__body">{{ t('settings.body') }}</p>
      </div>
      <span v-if="props.recommendedPreset" class="app-chip app-chip--accent">
        {{ t('settings.recommendationPrefix') }}: {{ t(`app.preset.${props.recommendedPreset}`) }}
      </span>
    </div>

    <dl class="settings-metrics">
      <div v-for="metric in settingsMetrics" :key="metric.label">
        <dt>{{ metric.label }}</dt>
        <dd>{{ metric.value }}</dd>
      </div>
    </dl>

    <div class="preset-switch" role="radiogroup" :aria-label="t('settings.presetGroupLabel')" :aria-disabled="props.disabled ? 'true' : 'false'">
      <button
        v-for="(option, index) in presetOptions"
        :key="option.value"
        class="preset-switch__option"
        :class="{ 'preset-switch__option--active': props.settings.preset === option.value }"
        type="button"
        role="radio"
        :aria-checked="props.settings.preset === option.value ? 'true' : 'false'"
        :tabindex="props.settings.preset === option.value ? 0 : -1"
        :data-preset-index="index"
        :disabled="props.disabled"
        @click="updateSetting('preset', option.value)"
        @keydown="handlePresetKeydown($event, index)"
      >
        <strong>{{ option.title }}</strong>
        <small>{{ option.description }}</small>
      </button>
    </div>

    <div class="slider-grid">
      <label class="range-control">
        <span>
          {{ t('settings.quality') }}
          <strong>{{ props.settings.imageQuality }}</strong>
        </span>
        <input
          type="range"
          min="40"
          max="90"
          step="2"
          :value="props.settings.imageQuality"
          :disabled="props.disabled"
          @input="updateSetting('imageQuality', Number(($event.target as HTMLInputElement).value))"
        />
      </label>

      <label class="range-control">
        <span>
          {{ t('settings.maxEdge') }}
          <strong>{{ props.settings.maxImageSizePx }} px</strong>
        </span>
        <input
          type="range"
          min="800"
          max="3200"
          step="100"
          :value="props.settings.maxImageSizePx"
          :disabled="props.disabled"
          @input="updateSetting('maxImageSizePx', Number(($event.target as HTMLInputElement).value))"
        />
      </label>
    </div>

    <div class="toggle-list">
      <label class="toggle-row">
        <input
          type="checkbox"
          :checked="props.settings.optimizeImages"
          :disabled="props.disabled"
          @change="updateSetting('optimizeImages', ($event.target as HTMLInputElement).checked)"
        />
        <span>
          <strong>{{ t('settings.optimizeImages') }}</strong>
          <small>{{ t('settings.optimizeImagesHint') }}</small>
        </span>
      </label>

      <label class="toggle-row">
        <input
          type="checkbox"
          :checked="props.settings.compressStreams"
          :disabled="props.disabled"
          @change="updateSetting('compressStreams', ($event.target as HTMLInputElement).checked)"
        />
        <span>
          <strong>{{ t('settings.compressStreams') }}</strong>
          <small>{{ t('settings.compressStreamsHint') }}</small>
        </span>
      </label>

      <label class="toggle-row">
        <input
          type="checkbox"
          :checked="props.settings.stripMetadata"
          :disabled="props.disabled"
          @change="updateSetting('stripMetadata', ($event.target as HTMLInputElement).checked)"
        />
        <span>
          <strong>{{ t('settings.stripMetadata') }}</strong>
          <small>{{ t('settings.stripMetadataHint') }}</small>
        </span>
      </label>
    </div>
  </section>
</template>

<style scoped>
.settings-panel,
.slider-grid,
.toggle-list,
.settings-metrics {
  display: grid;
  gap: var(--space-4);
}

.settings-panel__body,
.preset-switch__option small,
.toggle-row small,
.settings-metrics dt {
  margin: 0;
  color: var(--color-ink-muted);
}

.settings-metrics {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.settings-metrics div,
.preset-switch__option,
.range-control,
.toggle-row {
  border: 1px solid var(--color-line);
  border-radius: var(--radius-2);
  background: var(--color-surface-strong);
}

.settings-metrics div {
  padding: var(--space-3);
  background: linear-gradient(180deg, rgba(7, 17, 26, 0.42), rgba(18, 36, 54, 0.76));
}

.settings-metrics dd {
  margin: var(--space-1) 0 0;
  color: var(--color-ink-strong);
  font-weight: 600;
}

.preset-switch {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
}

.preset-switch__option {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-4);
  text-align: left;
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast),
    box-shadow var(--transition-fast);
}

.preset-switch__option:hover:not(:disabled) {
  border-color: rgba(255, 157, 87, 0.28);
  background: linear-gradient(135deg, rgba(255, 157, 87, 0.08), rgba(14, 29, 44, 0.94));
}

.preset-switch__option--active {
  border-color: var(--color-accent-strong);
  background: linear-gradient(135deg, var(--color-accent-soft), rgba(14, 29, 44, 0.94));
  box-shadow: 0 1rem 2rem rgba(4, 10, 19, 0.18);
}

.preset-switch__option:focus-visible {
  outline: none;
  border-color: var(--color-accent-strong);
  box-shadow: var(--shadow-focus);
}

.preset-switch__option strong,
.range-control strong,
.toggle-row strong {
  color: var(--color-ink-strong);
}

.slider-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.range-control {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
  transition:
    border-color var(--transition-fast),
    box-shadow var(--transition-fast),
    background-color var(--transition-fast);
}

.range-control:focus-within {
  border-color: var(--color-accent-strong);
  box-shadow: var(--shadow-focus);
}

.range-control span {
  display: flex;
  justify-content: space-between;
  gap: var(--space-2);
  color: var(--color-ink-strong);
  font-weight: 600;
}

input[type='range'],
.toggle-row input {
  accent-color: var(--color-accent-strong);
}

.toggle-row {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-4);
  align-items: flex-start;
  transition:
    border-color var(--transition-fast),
    box-shadow var(--transition-fast),
    background-color var(--transition-fast);
}

.toggle-row:hover {
  border-color: rgba(120, 211, 203, 0.28);
  background: rgba(12, 26, 39, 0.9);
}

.toggle-row:focus-within {
  border-color: var(--color-accent-strong);
  box-shadow: var(--shadow-focus);
}

.toggle-row span {
  display: grid;
  gap: var(--space-1);
}

.toggle-row input {
  margin-top: 0.2rem;
}

@media (max-width: 62rem) {
  .settings-metrics,
  .preset-switch,
  .slider-grid {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }
}
</style>
