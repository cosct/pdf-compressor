<script setup lang="ts">
/**
 * Compact preset bar for the main view — preset selection and target size
 * without leaving the queue. Full parameter editing lives in the settings
 * view (CompressionSettingsPanel).
 * 主视图的紧凑预设条 —— 不离开队列即可选择预设与目标大小。
 * 完整参数编辑在设置视图（CompressionSettingsPanel）。
 */
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { loadPresetProfiles } from '../config/presets'
import { normalizeSettings } from '../composables/usePdfCompressor'
import type { CompressionPreset, CompressionSettings, PresetProfileMap } from '../types/pdf'

const props = defineProps<{
  settings: CompressionSettings
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:settings': [value: CompressionSettings]
}>()

const { t } = useI18n()

const PRESET_ORDER: CompressionPreset[] = ['conservative', 'balanced', 'maximum', 'custom']

const profiles = ref<PresetProfileMap | null>(null)
const targetSizeInvalid = ref(false)

onMounted(async () => {
  try {
    profiles.value = await loadPresetProfiles()
  } catch {
    // Defaults suffice for picking a preset; profile load errors surface in
    // the settings view.
  }
})

function selectPreset(preset: CompressionPreset) {
  if (props.disabled) {
    return
  }
  const defaults = profiles.value?.[preset]
  emit(
    'update:settings',
    normalizeSettings({
      ...props.settings,
      ...(defaults ?? {}),
      preset,
    }),
  )
}

function updateTargetSizeMb(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim()
  if (!raw) {
    targetSizeInvalid.value = false
    emit('update:settings', normalizeSettings({ ...props.settings, targetFileSizeMb: null }))
    return
  }
  const parsed = Number(raw)
  if (Number.isFinite(parsed) && parsed >= 0.1 && parsed <= 2048) {
    targetSizeInvalid.value = false
    emit('update:settings', normalizeSettings({ ...props.settings, targetFileSizeMb: parsed }))
    return
  }
  targetSizeInvalid.value = true
}
</script>

<template>
  <section class="preset-bar" :aria-label="t('settings.presetGroupLabel')">
    <div class="preset-bar__presets" role="radiogroup" :aria-label="t('settings.presetGroupLabel')">
      <button
        v-for="preset in PRESET_ORDER"
        :key="preset"
        type="button"
        role="radio"
        :aria-checked="settings.preset === preset ? 'true' : 'false'"
        class="preset-bar__option"
        :class="{ 'preset-bar__option--active': settings.preset === preset }"
        :disabled="disabled"
        @click="selectPreset(preset)"
      >
        {{ t(`app.preset.${preset}`) }}
      </button>
    </div>

    <div class="preset-bar__target" :class="{ 'preset-bar__target--invalid': targetSizeInvalid }">
      <label class="preset-bar__target-label" for="preset-bar-target">{{ t('settings.targetSize') }}</label>
      <input
        id="preset-bar-target"
        type="text"
        inputmode="decimal"
        :value="settings.targetFileSizeMb ?? ''"
        :placeholder="t('settings.targetSizeOff')"
        :disabled="disabled"
        :aria-invalid="targetSizeInvalid ? 'true' : 'false'"
        @input="updateTargetSizeMb"
      />
    </div>
  </section>
</template>

<style scoped>
.preset-bar {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-10);
  padding-bottom: var(--fd-space-12);
  border-bottom: 1px solid var(--fd-stroke-soft);
}

.preset-bar__presets {
  display: flex;
  gap: var(--fd-space-4);
  padding: var(--fd-space-4);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-layer-1);
}

.preset-bar__option {
  flex: 1;
  min-height: var(--fd-control-height-xs);
  padding: var(--fd-space-4) var(--fd-space-6);
  border: 1px solid transparent;
  border-radius: calc(var(--fd-radius-sm) - 4px);
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  font: var(--fd-text-caption);
  white-space: nowrap;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard);
}

.preset-bar__option:hover:not(:disabled) {
  background: var(--fd-subtle-bg-hover);
}

.preset-bar__option--active {
  background: var(--fd-surface-raised);
  border-color: var(--fd-stroke-card);
  color: var(--fd-text-primary);
  box-shadow: var(--fd-shadow-4);
}

.preset-bar__option:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.preset-bar__option:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.preset-bar__target {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.preset-bar__target-label {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  white-space: nowrap;
}

.preset-bar__target input {
  flex: 1;
  min-width: 0;
  min-height: var(--fd-control-height-xs);
  padding: 0 var(--fd-space-8);
  border: 1px solid var(--fd-control-stroke);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-control-bg);
  outline: none;
  font: var(--fd-text-caption);
  font-variant-numeric: tabular-nums;
}

.preset-bar__target input:focus-visible {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.preset-bar__target--invalid input {
  border-color: var(--fd-danger-border);
}
</style>
