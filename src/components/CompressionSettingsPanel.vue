<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  clearUserPresetConfig,
  getDefaultPresetProfiles,
  hasUserPresetConfig,
  loadPresetProfiles,
  saveUserPresetProfile,
} from '../config/presets'
import type { AnalysisSummary, CompressionPreset, CompressionSettings } from '../types/pdf'
import { normalizeSettings } from '../composables/usePdfCompressor'
import {
  calculateMaxImageSizePx,
  clampImageQuality,
  clampMaxImageSizePercent,
  MAX_IMAGE_QUALITY,
  MAX_IMAGE_SIZE_PERCENT,
  MIN_IMAGE_QUALITY,
  MIN_IMAGE_SIZE_PERCENT,
  normalizeReferenceMaxImageEdgePx,
} from '../utils/compressionSettings'

const props = withDefaults(
  defineProps<{
    settings: CompressionSettings
    disabled: boolean
    canApplyToAll?: boolean
    applyToAllHint?: string | null
    recommendedPreset: CompressionPreset | null | undefined
    analysis?: AnalysisSummary | null
  }>(),
  {
    analysis: null,
    canApplyToAll: false,
    applyToAllHint: null,
  },
)

const emit = defineEmits<{
  'update:settings': [value: CompressionSettings]
  'apply-settings-to-all': []
  'preset-config-error': [error: unknown]
  'preset-config-saved': []
}>()

const { t } = useI18n()

/** Normalize with the analysis' reference edge as fallback (panel context). */
function normalizeWithAnalysis(settings: CompressionSettings): CompressionSettings {
  return normalizeSettings({
    ...settings,
    referenceMaxImageEdgePx:
      normalizeReferenceMaxImageEdgePx(settings.referenceMaxImageEdgePx) ??
      normalizeReferenceMaxImageEdgePx(props.analysis?.maxImageEdgePx),
  })
}

const defaultPresetProfiles = getDefaultPresetProfiles()
const presetProfiles = ref(getDefaultPresetProfiles())
const hasCustomPresets = ref(false)
const presetConfigBusy = ref(false)
const targetSizeInvalid = ref(false)
const maxImageEdgePx = computed(() => props.analysis?.maxImageEdgePx ?? 0)
const hasAnalysisResult = computed(() => maxImageEdgePx.value > 0)
const displayMaxImageSizePx = computed(() =>
  hasAnalysisResult.value
    ? calculateMaxImageSizePx(props.settings.maxImageSizePercent, maxImageEdgePx.value)
    : null,
)

const displayMaxImagePercent = computed(
  () => clampMaxImageSizePercent(props.settings.maxImageSizePercent),
)
const canResetPresets = computed(() => {
  const defaults = defaultPresetProfiles[props.settings.preset]

  return (
    hasCustomPresets.value ||
    clampImageQuality(props.settings.imageQuality) !== defaults.imageQuality ||
    displayMaxImagePercent.value !== defaults.maxImageSizePercent ||
    props.settings.optimizeImages !== defaults.optimizeImages ||
    props.settings.compressStreams !== defaults.compressStreams ||
    props.settings.stripMetadata !== defaults.stripMetadata ||
    props.settings.grayscale !== defaults.grayscale ||
    props.settings.bilevelCodec !== defaults.bilevelCodec ||
    props.settings.subsetFonts !== defaults.subsetFonts
  )
})

const presetOptions = computed<Array<{ value: CompressionPreset; title: string }>>(() => [
  { value: 'conservative', title: t('app.preset.conservative') },
  { value: 'balanced', title: t('app.preset.balanced') },
  { value: 'maximum', title: t('app.preset.maximum') },
  { value: 'custom', title: t('app.preset.custom') },
])

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
  emit(
    'update:settings',
    normalizeWithAnalysis({
      ...props.settings,
      grayscale: mode !== 'color',
      bilevelCodec: mode === 'bw' ? 'ccitt-g4' : 'jpeg',
    }),
  )
}

async function refreshPresetProfiles() {
  presetConfigBusy.value = true

  try {
    const [profiles, hasCustomConfig] = await Promise.all([
      loadPresetProfiles(),
      hasUserPresetConfig(),
    ])
    presetProfiles.value = profiles
    hasCustomPresets.value = hasCustomConfig
  } catch (error) {
    emit('preset-config-error', error)
  } finally {
    presetConfigBusy.value = false
  }
}

async function saveCurrentAsPreset() {
  const preset = props.settings.preset
  const percent = displayMaxImagePercent.value

  presetConfigBusy.value = true

  try {
    // Persist the full parameter set, not just quality/edge: presets are
    // meant to capture a complete compression recipe.
    await saveUserPresetProfile(preset, {
      imageQuality: clampImageQuality(props.settings.imageQuality),
      maxImageSizePercent: percent,
      optimizeImages: props.settings.optimizeImages,
      compressStreams: props.settings.compressStreams,
      stripMetadata: props.settings.stripMetadata,
      grayscale: props.settings.grayscale,
      bilevelCodec: props.settings.bilevelCodec,
      subsetFonts: props.settings.subsetFonts,
    })
    await refreshPresetProfiles()
    emit('preset-config-saved')
  } catch (error) {
    emit('preset-config-error', error)
  } finally {
    presetConfigBusy.value = false
  }
}

async function resetToDefaults() {
  presetConfigBusy.value = true

  try {
    if (hasCustomPresets.value) {
      await clearUserPresetConfig()
    }

    const defaults = getDefaultPresetProfiles()
    const nextPreset = props.settings.preset

    presetProfiles.value = defaults
    hasCustomPresets.value = false

    emit('update:settings', normalizeWithAnalysis({
      ...props.settings,
      preset: nextPreset,
      ...defaults[nextPreset],
    }))
  } catch (error) {
    emit('preset-config-error', error)
  } finally {
    presetConfigBusy.value = false
  }
}

onMounted(() => {
  void refreshPresetProfiles()
})

function applyLocalPresetPercent(preset: CompressionPreset, percent: number) {
  presetProfiles.value = {
    ...presetProfiles.value,
    [preset]: {
      ...presetProfiles.value[preset],
      maxImageSizePercent: clampMaxImageSizePercent(percent),
    },
  }
}

function selectPreset(value: CompressionPreset) {
  const defaults = presetProfiles.value[value]
  emit('update:settings', normalizeWithAnalysis({
    ...props.settings,
    ...defaults,
    preset: value,
  }))
}

function updateMaxImageSizePercent(percent: number) {
  const normalizedPercent = clampMaxImageSizePercent(percent)
  applyLocalPresetPercent(props.settings.preset, normalizedPercent)
  emit('update:settings', normalizeWithAnalysis({
    ...props.settings,
    maxImageSizePercent: normalizedPercent,
  }))
}

function updateSetting<K extends keyof CompressionSettings>(key: K, value: CompressionSettings[K]) {
  emit('update:settings', normalizeWithAnalysis({ ...props.settings, [key]: value }))
}

function updateTargetSizeMb(raw: string) {
  const trimmed = raw.trim()
  if (!trimmed) {
    targetSizeInvalid.value = false
    updateSetting('targetFileSizeMb', null)
    return
  }

  const parsed = Number(trimmed)
  if (Number.isFinite(parsed) && parsed > 0) {
    targetSizeInvalid.value = false
    updateSetting('targetFileSizeMb', parsed)
    return
  }

  // Reject with visible feedback instead of silently switching to "Off".
  targetSizeInvalid.value = true
  updateSetting('targetFileSizeMb', null)
}

function handleTargetSizeInput(raw: string) {
  if (targetSizeInvalid.value && raw.trim()) {
    targetSizeInvalid.value = false
  }
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
  selectPreset(presetOptions.value[nextIndex].value)

  const currentTarget = event.currentTarget
  if (!(currentTarget instanceof HTMLElement)) {
    return
  }

  const nextPresetButton = currentTarget.parentElement?.querySelector<HTMLButtonElement>(
    `[data-preset-index="${nextIndex}"]`,
  )
  nextPresetButton?.focus()
}

function presetSnapshotLabel(preset: CompressionPreset): string {
  const snapshot = presetProfiles.value[preset] ?? defaultPresetProfiles[preset]
  return `${snapshot.imageQuality}/${snapshot.maxImageSizePercent}%`
}
</script>

<template>
  <section class="settings-dock">
    <div class="dock-head">
      <div class="panel-header">
        <svg class="panel-header__icon" width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true">
          <path d="M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" stroke="var(--fd-accent)" stroke-width="1.3"/>
          <path d="M16.16 12.42a1.27 1.27 0 0 0 .25 1.4l.05.05a1.54 1.54 0 1 1-2.18 2.18l-.05-.05a1.27 1.27 0 0 0-1.4-.25 1.27 1.27 0 0 0-.77 1.16v.14a1.54 1.54 0 0 1-3.08 0v-.07a1.27 1.27 0 0 0-.83-1.16 1.27 1.27 0 0 0-1.4.25l-.05.05A1.54 1.54 0 1 1 4.52 14l.05-.05a1.27 1.27 0 0 0 .25-1.4A1.27 1.27 0 0 0 3.66 11.78h-.14a1.54 1.54 0 0 1 0-3.08h.07a1.27 1.27 0 0 0 1.16-.83 1.27 1.27 0 0 0-.25-1.4L4.45 6.42A1.54 1.54 0 1 1 6.63 4.24l.05.05a1.27 1.27 0 0 0 1.4.25h.06a1.27 1.27 0 0 0 .77-1.16v-.14a1.54 1.54 0 0 1 3.08 0v.07a1.27 1.27 0 0 0 .77 1.16 1.27 1.27 0 0 0 1.4-.25l.05-.05a1.54 1.54 0 1 1 2.18 2.18l-.05.05a1.27 1.27 0 0 0-.25 1.4v.06a1.27 1.27 0 0 0 1.16.77h.14a1.54 1.54 0 0 1 0 3.08h-.07a1.27 1.27 0 0 0-1.16.77Z" stroke="var(--fd-text-tertiary)" stroke-width="1.1" fill="none"/>
        </svg>
        <div class="panel-header__copy">
          <h2>{{ t('settings.eyebrow') }}</h2>
        </div>
      </div>

      <div class="preset-actions">
        <button
          class="fd-button fd-button--subtle"
          type="button"
          :disabled="props.disabled || presetConfigBusy"
          @click="saveCurrentAsPreset"
        >
          {{ t('settings.savePreset') }}
        </button>
        <button
          class="fd-button fd-button--subtle"
          type="button"
          :disabled="presetConfigBusy || !canResetPresets"
          @click="resetToDefaults"
        >
          {{ t('settings.resetPresets') }}
        </button>
        <button
          class="fd-button fd-button--subtle"
          type="button"
          :disabled="!props.canApplyToAll"
          :title="props.canApplyToAll ? undefined : (props.applyToAllHint ?? undefined)"
          @click="emit('apply-settings-to-all')"
        >
          {{ t('settings.applyToAll') }}
        </button>
      </div>
    </div>

    <div
      class="preset-ribbon"
      role="radiogroup"
      :aria-label="t('settings.presetGroupLabel')"
      :aria-disabled="props.disabled ? 'true' : 'false'"
    >
      <button
        v-for="(option, index) in presetOptions"
        :key="option.value"
        class="preset-pill"
        :class="{ 'preset-pill--active': props.settings.preset === option.value }"
        type="button"
        role="radio"
        :aria-checked="props.settings.preset === option.value ? 'true' : 'false'"
        :tabindex="props.settings.preset === option.value ? 0 : -1"
        :data-preset-index="index"
        :disabled="props.disabled"
        @click="selectPreset(option.value)"
        @keydown="handlePresetKeydown($event, index)"
      >
        <span class="preset-pill__head">
          <span class="preset-pill__title-line">
            <span class="preset-pill__title">{{ option.title }}</span>
            <span class="preset-pill__meta">{{ presetSnapshotLabel(option.value) }}</span>
          </span>
          <span v-if="props.recommendedPreset === option.value" class="preset-pill__badge">
            {{ t('settings.recommendedBadge') }}
          </span>
        </span>
      </button>
    </div>

    <details class="advanced-panel" open>
      <summary
        :class="{ 'advanced-panel__summary-lock': props.disabled }"
        @click="props.disabled ? $event.preventDefault() : undefined"
      >
        <span class="advanced-panel__summary">
          <strong>{{ t('settings.advancedToggle') }}</strong>
        </span>
        <svg class="advanced-panel__chevron" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
          <path d="M3 4.5l3 3 3-3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </summary>

      <div class="advanced-content">
        <div class="slider-row">
          <label class="slider-control">
            <span class="slider-label">
              <span>{{ t('settings.quality') }}</span>
              <span class="slider-label__value">
                <strong>{{ props.settings.imageQuality }}</strong>
              </span>
            </span>
            <input
              type="range"
              :min="MIN_IMAGE_QUALITY"
              :max="MAX_IMAGE_QUALITY"
              step="1"
              :value="props.settings.imageQuality"
              :disabled="props.disabled"
              @input="updateSetting('imageQuality', Number(($event.target as HTMLInputElement).value))"
            />
          </label>

          <label class="slider-control">
            <span class="slider-label">
              <span>{{ t('settings.maxEdge') }}</span>
              <span class="slider-label__value">
                <strong>{{ displayMaxImagePercent }}%</strong>
                <span v-if="displayMaxImageSizePx !== null" class="slider-label__hint">
                  {{ displayMaxImageSizePx }} px
                </span>
              </span>
            </span>
            <input
              type="range"
              :min="MIN_IMAGE_SIZE_PERCENT"
              :max="MAX_IMAGE_SIZE_PERCENT"
              step="1"
              :value="displayMaxImagePercent"
              :disabled="props.disabled"
              @input="updateMaxImageSizePercent(Number(($event.target as HTMLInputElement).value))"
            />
          </label>
        </div>

        <div class="target-size-row">
          <label class="target-size-control">
            <span class="slider-label">
              <span>{{ t('settings.targetSize') }}</span>
              <span class="slider-label__value">
                <strong v-if="props.settings.targetFileSizeMb">
                  {{ props.settings.targetFileSizeMb }} MB
                </strong>
                <span v-else class="slider-label__hint">{{ t('settings.targetSizeOff') }}</span>
              </span>
            </span>
            <input
              class="target-size-input"
              :class="{ 'target-size-input--invalid': targetSizeInvalid }"
              type="number"
              :min="0.1"
              :max="2048"
              step="0.1"
              inputmode="decimal"
              :placeholder="t('settings.targetSizeHint')"
              :value="props.settings.targetFileSizeMb ?? ''"
              :aria-invalid="targetSizeInvalid ? 'true' : undefined"
              :disabled="props.disabled"
              @input="handleTargetSizeInput(($event.target as HTMLInputElement).value)"
              @change="updateTargetSizeMb(($event.target as HTMLInputElement).value)"
            />
          </label>
          <p v-if="targetSizeInvalid" class="target-size-warning" role="alert">
            {{ t('settings.targetSizeInvalid') }}
          </p>
        </div>

        <div class="color-mode-row" role="radiogroup" :aria-label="t('settings.colorMode')">
          <span class="color-mode-row__label">{{ t('settings.colorMode') }}</span>
          <div class="color-mode-seg">
            <button
              v-for="option in colorModeOptions"
              :key="option.value"
              class="color-mode-seg__item"
              :class="{ 'color-mode-seg__item--active': colorMode === option.value }"
              type="button"
              role="radio"
              :aria-checked="colorMode === option.value ? 'true' : 'false'"
              :tabindex="colorMode === option.value ? 0 : -1"
              :disabled="props.disabled"
              @click="selectColorMode(option.value)"
            >
              {{ option.title }}
            </button>
          </div>
        </div>

        <div class="toggle-list">
          <label class="toggle-chip">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.optimizeImages"
                :disabled="props.disabled"
                @change="updateSetting('optimizeImages', ($event.target as HTMLInputElement).checked)"
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.optimizeImages') }}</span>
          </label>

          <label class="toggle-chip">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.compressStreams"
                :disabled="props.disabled"
                @change="updateSetting('compressStreams', ($event.target as HTMLInputElement).checked)"
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.compressStreams') }}</span>
          </label>

          <label class="toggle-chip">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.stripMetadata"
                :disabled="props.disabled"
                @change="updateSetting('stripMetadata', ($event.target as HTMLInputElement).checked)"
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.stripMetadata') }}</span>
          </label>

          <label class="toggle-chip">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.subsetFonts"
                :disabled="props.disabled"
                @change="updateSetting('subsetFonts', ($event.target as HTMLInputElement).checked)"
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.subsetFonts') }}</span>
          </label>
        </div>
      </div>
    </details>
  </section>
</template>

<style scoped>
.settings-dock {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-8);
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
}

.dock-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--fd-space-8);
}

.panel-header__copy {
  display: flex;
  flex-direction: column;
}

.preset-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--fd-space-6);
}

.preset-actions .fd-button {
  min-height: 26px;
  padding: 0 8px;
  border-radius: 12px;
  font: var(--fd-text-caption);
}

.preset-ribbon {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fd-space-6);
}

.preset-pill {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: flex-start;
  gap: 6px;
  min-height: 46px;
  padding: 8px 8px;
  border: 1px solid var(--fd-control-stroke);
  border-radius: 14px;
  background: var(--fd-layer-2);
  color: var(--fd-text-primary);
  cursor: pointer;
  text-align: left;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    border-color var(--fd-duration-fast) var(--fd-easing-standard),
    box-shadow var(--fd-duration-fast) var(--fd-easing-standard);
}

.preset-pill:hover:not(:disabled) {
  background: var(--fd-control-bg-hover);
}

.preset-pill--active {
  border-color: var(--fd-accent-border);
  background: var(--fd-accent-subtle);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--fd-accent) 18%, transparent);
}

.preset-pill:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.preset-pill:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.preset-pill__head {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
}

.preset-pill__title-line {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 6px;
  width: 100%;
}

.preset-pill__title {
  min-width: 0;
  font: 600 12px/1.2 var(--fd-font-family);
  letter-spacing: -0.01em;
  white-space: nowrap;
  overflow: visible;
  text-overflow: clip;
}

.preset-pill__meta {
  color: var(--fd-text-secondary);
  font: 600 10px/1.2 var(--fd-font-family);
  white-space: nowrap;
  text-align: right;
}

.preset-pill__badge {
  display: inline-flex;
  align-items: center;
  align-self: flex-start;
  min-height: 16px;
  padding: 0 6px;
  border: 1px solid var(--fd-accent-border);
  border-radius: var(--fd-radius-full);
  background: var(--fd-accent);
  color: var(--fd-accent-text);
  font: 700 10px/1 var(--fd-font-family);
  white-space: nowrap;
  flex-shrink: 0;
}

.advanced-panel {
  display: flex;
  flex: 0 0 auto;
  flex-direction: column;
  min-height: 0;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 18px;
  overflow: hidden;
  background: color-mix(in srgb, var(--fd-layer-2) 92%, transparent);
}

.advanced-panel[open] {
  flex: 1 1 auto;
}

.advanced-panel:not([open]) {
  overflow: hidden;
}

.advanced-panel summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-8);
  min-height: 36px;
  padding: 6px 12px;
  cursor: pointer;
  list-style: none;
  transition: background-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.advanced-panel summary:hover {
  background: var(--fd-subtle-bg-hover);
}

.advanced-panel__summary-lock {
  cursor: not-allowed;
}

.advanced-panel__summary-lock:hover {
  background: transparent;
}

.advanced-panel summary::-webkit-details-marker {
  display: none;
}

.advanced-panel__summary {
  display: flex;
  align-items: center;
  min-height: 24px;
}

.advanced-panel__summary strong {
  font: var(--fd-text-body-strong);
}




.advanced-panel__chevron {
  color: var(--fd-text-tertiary);
  transition: transform var(--fd-duration-fast) var(--fd-easing-standard);
}

.advanced-panel[open] .advanced-panel__chevron {
  transform: rotate(180deg);
}

.advanced-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: var(--fd-space-8);
  padding: 8px 12px 12px;
  min-height: 0;
  overflow: auto;
}

.advanced-panel:not([open]) .advanced-content {
  display: none;
}

.slider-row {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-8);
}

.slider-control {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
  padding: 10px 12px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 14px;
  background: color-mix(in srgb, var(--fd-layer-1) 88%, transparent);
}

.slider-label {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: var(--fd-space-4);
  font: var(--fd-text-caption);
  color: var(--fd-text-primary);
  font-weight: 500;
}

.slider-label__value {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--fd-space-4);
  min-width: 0;
  text-align: right;
}

.slider-label__hint {
  color: var(--fd-text-tertiary);
  font-weight: 400;
}

input[type='range'] {
  width: 100%;
  accent-color: var(--fd-accent);
  cursor: pointer;
}

input[type='range']:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.target-size-row {
  display: grid;
  grid-template-columns: 1fr;
}

.target-size-control {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
  padding: 10px 12px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 14px;
  background: color-mix(in srgb, var(--fd-layer-1) 88%, transparent);
}

.target-size-input {
  width: 100%;
  padding: 4px 8px;
  border: 1px solid var(--fd-control-stroke);
  border-radius: 8px;
  background: var(--fd-layer-2);
  color: var(--fd-text-primary);
  font: var(--fd-text-body);
}

.target-size-input:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.target-size-input--invalid {
  border-color: var(--fd-danger-border);
}

.target-size-input--invalid:focus {
  outline: none;
  border-color: var(--fd-danger);
}

.target-size-warning {
  margin: 0;
  padding: 0 12px;
  color: var(--fd-danger);
  font: var(--fd-text-caption);
}

.toggle-list {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-6);
}

.color-mode-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-8);
}

.color-mode-row__label {
  font: var(--fd-text-body);
  font-weight: 500;
  color: var(--fd-text-primary);
}

.color-mode-seg {
  display: inline-flex;
  border: 1px solid var(--fd-control-stroke);
  border-radius: 14px;
  background: var(--fd-layer-2);
  overflow: hidden;
}

.color-mode-seg__item {
  min-height: 30px;
  padding: 4px 14px;
  border: none;
  background: transparent;
  color: var(--fd-text-secondary);
  font: var(--fd-text-caption);
  font-weight: 600;
  cursor: pointer;
  transition: background-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.color-mode-seg__item:hover:not(:disabled):not(.color-mode-seg__item--active) {
  background: var(--fd-control-bg-hover);
}

.color-mode-seg__item--active {
  background: var(--fd-accent-subtle);
  color: var(--fd-text-primary);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--fd-accent) 18%, transparent);
}

.color-mode-seg__item:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.color-mode-seg__item:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.toggle-chip {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  min-height: 38px;
  padding: 0 10px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 14px;
  background: color-mix(in srgb, var(--fd-layer-1) 88%, transparent);
  cursor: pointer;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    border-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.toggle-chip:hover {
  background: var(--fd-subtle-bg-hover);
}

.toggle-chip__label {
  font: var(--fd-text-body);
  font-weight: 500;
  color: var(--fd-text-primary);
}

@media (max-width: 1080px) {
  .dock-head {
    flex-direction: column;
    align-items: flex-start;
  }

  .preset-actions {
    justify-content: flex-start;
  }

  .preset-ribbon {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .toggle-list {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 900px) {
  .slider-row {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 768px) {
  .preset-ribbon {
    grid-template-columns: 1fr;
  }
}
</style>
