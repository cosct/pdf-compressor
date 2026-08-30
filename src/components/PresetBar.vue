<script setup lang="ts">
/**
 * Compact control bar for the main view — compression mode (presets plus
 * target-size as a peer mode with slider/input/unit controls), color mode,
 * and apply-to-all without leaving the queue. Full parameter editing lives
 * in the settings view (CompressionSettingsPanel).
 * 主视图的紧凑控制条 —— 压缩模式（预设与目标大小并列，目标大小带滑块/
 * 数值/单位控制）、色彩模式与应用到全部。完整参数编辑在设置视图。
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { loadPresetProfiles } from '../config/presets'
import { normalizeSettings } from '../composables/usePdfCompressor'
import { clampMaxImageSizePercent } from '../utils/compressionSettings'
import UnitSelect from './UnitSelect.vue'
import type {
  CompressionPreset,
  CompressionSettings,
  PresetProfileMap,
} from '../types/pdf'

const props = withDefaults(
  defineProps<{
    settings: CompressionSettings
    disabled?: boolean
    canApplyToAll?: boolean
    applyToAllHint?: string | null
    /** Analysis-recommended preset — marked on its option button. */
    recommendedPreset?: CompressionPreset | null
  }>(),
  {
    disabled: false,
    canApplyToAll: false,
    applyToAllHint: null,
    recommendedPreset: null,
  },
)

const emit = defineEmits<{
  'update:settings': [value: CompressionSettings]
  'apply-settings-to-all': []
}>()

const { t } = useI18n()

const PRESET_ORDER: CompressionPreset[] = ['conservative', 'balanced', 'maximum', 'custom']
const DEFAULT_TARGET_MB = 5
/** Stored value stays in MB; the unit only shapes the input display. */
const TARGET_UNITS = ['KB', 'MB', 'GB'] as const

const profiles = ref<PresetProfileMap | null>(null)
const targetUnit = ref<string>('MB')
const targetInvalid = ref(false)

/** Target-size is a compression mode: set value ⇒ active, presets clear it. */
const isTargetMode = computed(() => props.settings.targetFileSizeMb != null)

type ModeValue = CompressionPreset | 'target'

const modeOptions = computed<Array<{ value: ModeValue; title: string; active: boolean }>>(() => [
  ...PRESET_ORDER.map((preset) => ({
    value: preset as ModeValue,
    title: t(`app.preset.${preset}`),
    active: !isTargetMode.value && props.settings.preset === preset,
  })),
  {
    value: 'target',
    title: t('settings.targetMode'),
    active: isTargetMode.value,
  },
])

// --- Read-only parameter echo of the selected mode --------------------------
// Mirrors the quick-compress panel, but reflects the selected job's actual
// settings: switching a mode adopts the preset profile's values, and manual
// tweaks in the settings view show through. Hidden in target mode — the
// engine searches parameters itself to fit the budget.

const presetParamsView = computed(() => ({
  imageQuality: props.settings.imageQuality,
  maxImageSizePercent: clampMaxImageSizePercent(props.settings.maxImageSizePercent),
}))

// --- Target-size unit conversion -------------------------------------------

function mbToUnit(mb: number): number {
  switch (targetUnit.value) {
    case 'KB':
      return mb * 1024
    case 'GB':
      return mb / 1024
    default:
      return mb
  }
}

function unitToMb(value: number): number {
  switch (targetUnit.value) {
    case 'KB':
      return value / 1024
    case 'GB':
      return value * 1024
    default:
      return value
  }
}

const targetDisplayValue = computed(() => {
  const mb = props.settings.targetFileSizeMb
  return mb == null ? '' : String(Math.round(mbToUnit(mb) * 100) / 100)
})

// --- Mode / value updates ---------------------------------------------------

function emitSettings(patch: Partial<CompressionSettings>) {
  emit('update:settings', normalizeSettings({ ...props.settings, ...patch }))
}

function selectMode(mode: ModeValue) {
  if (props.disabled) {
    return
  }
  if (mode === 'target') {
    emitSettings({ targetFileSizeMb: props.settings.targetFileSizeMb ?? DEFAULT_TARGET_MB })
    return
  }
  const defaults = profiles.value?.[mode]
  emit(
    'update:settings',
    normalizeSettings({
      ...props.settings,
      ...(defaults ?? {}),
      preset: mode,
      targetFileSizeMb: null,
    }),
  )
}

function updateTargetFromInput(raw: string) {
  const parsed = Number(raw)
  const mb = Number.isFinite(parsed) ? unitToMb(parsed) : Number.NaN

  if (raw && Number.isFinite(mb) && mb > 0 && mb <= 2048) {
    targetInvalid.value = false
    emitSettings({ targetFileSizeMb: mb })
    return
  }
  targetInvalid.value = true
}

/**
 * Commit on change (blur/Enter) instead of every keystroke: binding the
 * committed value back on `input` would clobber decimal intermediate states
 * like "5." while the user is still typing.
 */
function commitTargetInput(event: Event) {
  updateTargetFromInput((event.target as HTMLInputElement).value.trim())
}

function handleTargetInput() {
  if (targetInvalid.value) {
    targetInvalid.value = false
  }
}

// --- Color mode -------------------------------------------------------------

type ColorMode = 'color' | 'gray' | 'bw'

/** The grayscale/bilevel pair expressed as one UI choice. */
const colorMode = computed<ColorMode>(() => {
  if (props.settings.bilevelCodec === 'ccitt-g4') {
    return 'bw'
  }
  return props.settings.grayscale ? 'gray' : 'color'
})

const colorModeOptions = computed<Array<{ value: ColorMode; title: string }>>(() => [
  { value: 'color', title: t('settings.colorModeColor') },
  { value: 'gray', title: t('settings.colorModeGray') },
  { value: 'bw', title: t('settings.colorModeBw') },
])

function selectColorMode(mode: ColorMode) {
  if (props.disabled) {
    return
  }
  emitSettings({
    grayscale: mode !== 'color',
    bilevelCodec: mode === 'bw' ? 'ccitt-g4' : 'jpeg',
  })
}

onMounted(async () => {
  try {
    profiles.value = await loadPresetProfiles()
  } catch {
    // Defaults suffice for picking a preset; profile load errors surface in
    // the settings view.
  }
})
</script>

<template>
  <section class="preset-bar" :aria-label="t('settings.presetGroupLabel')">
    <div class="preset-bar__group">
      <span class="preset-bar__section-label">{{ t('settings.compressionMode') }}</span>
      <div class="preset-bar__presets" role="radiogroup" :aria-label="t('settings.presetGroupLabel')">
        <button
          v-for="option in modeOptions"
          :key="option.value"
          type="button"
          role="radio"
          :aria-checked="option.active ? 'true' : 'false'"
          class="preset-bar__option"
          :class="{ 'preset-bar__option--active': option.active }"
          :disabled="disabled"
          :title="recommendedPreset === option.value ? t('settings.recommendedBadge') : undefined"
          @click="selectMode(option.value)"
        >
          {{ option.title }}
          <span
            v-if="recommendedPreset === option.value"
            class="preset-bar__recommend-tag"
            aria-hidden="true"
          >
            {{ t('settings.recommendedShort') }}
          </span>
        </button>
      </div>

      <!-- Read-only echo of the selected preset's parameters. -->
      <div v-if="!isTargetMode" class="preset-bar__params" :aria-label="t('quick.params')">
        <div class="preset-bar__param">
          <span class="preset-bar__param-label">{{ t('settings.quality') }}</span>
          <span class="preset-bar__param-value">{{ presetParamsView.imageQuality }}</span>
        </div>
        <div class="preset-bar__param">
          <span class="preset-bar__param-label">{{ t('settings.maxEdge') }}</span>
          <span class="preset-bar__param-value">{{ presetParamsView.maxImageSizePercent }}%</span>
        </div>
      </div>
    </div>

    <div v-if="isTargetMode" class="preset-bar__target-controls">
      <div class="preset-bar__target-entry" :class="{ 'preset-bar__target-entry--invalid': targetInvalid }">
        <input
          type="text"
          inputmode="decimal"
          :value="targetDisplayValue"
          :disabled="disabled"
          :aria-label="t('settings.targetMode')"
          :aria-invalid="targetInvalid ? 'true' : 'false'"
          :title="targetInvalid ? t('settings.targetSizeInvalid') : undefined"
          @input="handleTargetInput"
          @change="commitTargetInput"
          @keydown.enter.prevent="commitTargetInput"
        />
        <UnitSelect
          v-model="targetUnit"
          :options="TARGET_UNITS"
          :disabled="disabled"
          :aria-label="t('settings.targetSizeUnit')"
        />
      </div>
      <p v-if="targetInvalid" class="preset-bar__target-warning" role="alert">
        {{ t('settings.targetSizeInvalid') }}
      </p>
    </div>

    <div class="preset-bar__group">
      <span class="preset-bar__section-label">{{ t('settings.colorMode') }}</span>
      <div class="preset-bar__seg" role="radiogroup" :aria-label="t('settings.colorMode')">
        <button
          v-for="option in colorModeOptions"
          :key="option.value"
          type="button"
          role="radio"
          :aria-checked="colorMode === option.value ? 'true' : 'false'"
          class="preset-bar__seg-item"
          :class="{ 'preset-bar__seg-item--active': colorMode === option.value }"
          :disabled="disabled"
          @click="selectColorMode(option.value)"
        >
          {{ option.title }}
        </button>
      </div>
    </div>

    <button
      class="fd-button preset-bar__apply-all"
      type="button"
      :disabled="disabled || !canApplyToAll"
      :title="canApplyToAll ? undefined : (applyToAllHint ?? undefined)"
      @click="emit('apply-settings-to-all')"
    >
      {{ t('settings.applyToAll') }}
    </button>
  </section>
</template>

<style scoped>
.preset-bar {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-12);
  padding-bottom: var(--fd-space-12);
  border-bottom: 1px solid var(--fd-stroke-soft);
}

.preset-bar__group {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
}

.preset-bar__section-label {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  line-height: 1.2;
}

/* Read-only parameter echo: two compact chips under the mode selector. */
.preset-bar__params {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-6);
  margin-top: 3px;
}

.preset-bar__param {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-6);
  min-height: 26px;
  padding: 0 var(--fd-space-10);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: color-mix(in srgb, var(--fd-layer-1) 70%, transparent);
}

.preset-bar__param-label {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  white-space: nowrap;
}

.preset-bar__param-value {
  font: 600 13px/18px var(--fd-font-family);
  font-variant-numeric: tabular-nums;
  color: var(--fd-text-primary);
  white-space: nowrap;
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
  position: relative;
  flex: 1;
  min-height: var(--fd-control-height-sm);
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

/* Floating corner tag naming the analysis-recommended preset. */
.preset-bar__recommend-tag {
  position: absolute;
  top: -7px;
  right: 2px;
  display: inline-flex;
  align-items: center;
  min-height: 13px;
  padding: 0 4px;
  border-radius: var(--fd-radius-full);
  background: var(--fd-accent);
  color: var(--fd-accent-text);
  font: 700 9px/1 var(--fd-font-family);
  letter-spacing: 0.02em;
  pointer-events: none;
}

/* Target-size mode controls: numeric entry with a unit picker. */
.preset-bar__target-controls {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-4);
}

.preset-bar__target-entry {
  display: flex;
  align-items: center;
  border: 1px solid var(--fd-control-stroke);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-control-bg);
}

.preset-bar__target-entry:focus-within {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.preset-bar__target-entry--invalid {
  border-color: var(--fd-danger-border);
}

.preset-bar__target-entry input {
  flex: 1;
  min-width: 0;
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-8);
  border: none;
  background: transparent;
  outline: none;
  font: var(--fd-text-caption);
  font-variant-numeric: tabular-nums;
  text-align: left;
}

.preset-bar__target-warning {
  margin: 0;
  color: var(--fd-danger);
  font: var(--fd-text-caption);
}

/* Color mode: label on its own line above the segmented control. */
.preset-bar__seg {
  display: flex;
  flex: 1;
  gap: var(--fd-space-4);
  padding: var(--fd-space-4);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-layer-1);
}

.preset-bar__seg-item {
  flex: 1;
  min-height: var(--fd-control-height-sm);
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

.preset-bar__seg-item:hover:not(:disabled) {
  background: var(--fd-subtle-bg-hover);
}

.preset-bar__seg-item--active {
  background: var(--fd-surface-raised);
  border-color: var(--fd-stroke-card);
  color: var(--fd-text-primary);
  box-shadow: var(--fd-shadow-4);
}

.preset-bar__seg-item:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.preset-bar__seg-item:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.preset-bar__apply-all {
  width: 100%;
  min-height: 36px;
  border-radius: var(--fd-radius-sm);
  font: var(--fd-text-body);
  font-weight: 500;
}
</style>
