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
    analysis?: AnalysisSummary | null
  }>(),
  {
    analysis: null,
  },
)

const emit = defineEmits<{
  'update:settings': [value: CompressionSettings]
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
const displayMaxImagePercent = computed(() =>
  clampMaxImageSizePercent(props.settings.maxImageSizePercent),
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

    emit(
      'update:settings',
      normalizeWithAnalysis({
        ...props.settings,
        preset: nextPreset,
        ...defaults[nextPreset],
      }),
    )
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
  emit(
    'update:settings',
    normalizeWithAnalysis({
      ...props.settings,
      ...defaults,
      preset: value,
    }),
  )
}

function updateMaxImageSizePercent(percent: number) {
  const normalizedPercent = clampMaxImageSizePercent(percent)
  applyLocalPresetPercent(props.settings.preset, normalizedPercent)
  emit(
    'update:settings',
    normalizeWithAnalysis({
      ...props.settings,
      maxImageSizePercent: normalizedPercent,
    }),
  )
}

function updateSetting<K extends keyof CompressionSettings>(key: K, value: CompressionSettings[K]) {
  emit('update:settings', normalizeWithAnalysis({ ...props.settings, [key]: value }))
}

// --- Numeric entry beside each slider --------------------------------------
// Commits on change (blur/Enter): rebinding the committed value on every
// keystroke would clobber intermediate states like "7" while typing "72".

const qualityInvalid = ref(false)

function commitQuality(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim()
  const parsed = Number(raw)
  if (
    raw !== '' &&
    Number.isInteger(parsed) &&
    parsed >= MIN_IMAGE_QUALITY &&
    parsed <= MAX_IMAGE_QUALITY
  ) {
    qualityInvalid.value = false
    updateSetting('imageQuality', parsed)
    return
  }
  qualityInvalid.value = true
}

const percentInvalid = ref(false)

function commitMaxImagePercent(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim()
  const parsed = Number(raw)
  if (
    raw !== '' &&
    Number.isInteger(parsed) &&
    parsed >= MIN_IMAGE_SIZE_PERCENT &&
    parsed <= MAX_IMAGE_SIZE_PERCENT
  ) {
    percentInvalid.value = false
    updateMaxImageSizePercent(parsed)
    return
  }
  percentInvalid.value = true
}

/**
 * The advanced section is a plain toggle (not `<details>`): details wraps its
 * content in an internal box that flex stretching cannot reach, which broke
 * the card's height distribution on tall windows.
 */
const advancedOpen = ref(true)

function toggleAdvanced() {
  if (!props.disabled) {
    advancedOpen.value = !advancedOpen.value
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
        <svg
          class="panel-header__icon"
          width="20"
          height="20"
          viewBox="0 0 20 20"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M3 6.5h5.5M12.5 6.5H17M3 13.5h2.5M9.5 13.5H17"
            stroke="var(--fd-text-tertiary)"
            stroke-width="1.4"
            stroke-linecap="round"
          />
          <circle cx="10" cy="6.5" r="2.3" stroke="var(--fd-accent)" stroke-width="1.4" />
          <circle cx="7" cy="13.5" r="2.3" stroke="var(--fd-accent)" stroke-width="1.4" />
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
        </span>
      </button>
    </div>

    <!-- Quality and max-edge lead the panel, one full-width row each; both
         the slider and a numeric field drive the same value. -->
    <div class="param-rows">
      <label class="slider-control">
        <span class="slider-label" :title="t('settings.qualityHint')">
          <span>{{ t('settings.quality') }}</span>
          <span class="slider-field" :class="{ 'slider-field--invalid': qualityInvalid }">
            <input
              type="number"
              :min="MIN_IMAGE_QUALITY"
              :max="MAX_IMAGE_QUALITY"
              step="1"
              inputmode="numeric"
              :value="props.settings.imageQuality"
              :disabled="props.disabled"
              :aria-label="t('settings.quality')"
              :aria-invalid="qualityInvalid ? 'true' : undefined"
              @input="qualityInvalid = false"
              @change="commitQuality"
              @keydown.enter.prevent="commitQuality"
            />
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
        <span class="slider-label" :title="t('settings.maxEdgeHint')">
          <span>{{ t('settings.maxEdge') }}</span>
          <span class="slider-field" :class="{ 'slider-field--invalid': percentInvalid }">
            <input
              type="number"
              :min="MIN_IMAGE_SIZE_PERCENT"
              :max="MAX_IMAGE_SIZE_PERCENT"
              step="1"
              inputmode="numeric"
              :value="displayMaxImagePercent"
              :disabled="props.disabled"
              :aria-label="t('settings.maxEdge')"
              :aria-invalid="percentInvalid ? 'true' : undefined"
              @input="percentInvalid = false"
              @change="commitMaxImagePercent"
              @keydown.enter.prevent="commitMaxImagePercent"
            />
            <span class="slider-field__unit">%</span>
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

    <div class="advanced-panel" :class="{ 'advanced-panel--open': advancedOpen }">
      <button
        class="advanced-panel__summary"
        :class="{ 'advanced-panel__summary-lock': props.disabled }"
        type="button"
        :aria-expanded="advancedOpen ? 'true' : 'false'"
        @click="toggleAdvanced"
      >
        <strong>{{ t('settings.advancedToggle') }}</strong>
        <svg
          class="advanced-panel__chevron"
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M3 4.5l3 3 3-3"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>

      <div v-show="advancedOpen" class="advanced-content">
        <div class="toggle-list">
          <label class="toggle-chip" :title="t('settings.optimizeImagesHint')">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.optimizeImages"
                :disabled="props.disabled"
                @change="
                  updateSetting('optimizeImages', ($event.target as HTMLInputElement).checked)
                "
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.optimizeImages') }}</span>
          </label>

          <label class="toggle-chip" :title="t('settings.compressStreamsHint')">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.compressStreams"
                :disabled="props.disabled"
                @change="
                  updateSetting('compressStreams', ($event.target as HTMLInputElement).checked)
                "
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.compressStreams') }}</span>
          </label>

          <label class="toggle-chip" :title="t('settings.stripMetadataHint')">
            <span class="fd-toggle">
              <input
                type="checkbox"
                :checked="props.settings.stripMetadata"
                :disabled="props.disabled"
                @change="
                  updateSetting('stripMetadata', ($event.target as HTMLInputElement).checked)
                "
              />
            </span>
            <span class="toggle-chip__label">{{ t('settings.stripMetadata') }}</span>
          </label>

          <label class="toggle-chip" :title="t('settings.subsetFontsHint')">
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
    </div>
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

.advanced-panel--open {
  flex: 1 1 auto;
}

.advanced-panel__summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-8);
  min-height: 36px;
  padding: 6px 12px;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  text-align: left;
  transition: background-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.advanced-panel__summary:hover {
  background: var(--fd-subtle-bg-hover);
}

.advanced-panel__summary-lock {
  cursor: not-allowed;
}

.advanced-panel__summary-lock:hover {
  background: transparent;
}

.advanced-panel__summary strong {
  font: var(--fd-text-body-strong);
}

.advanced-panel__chevron {
  color: var(--fd-text-tertiary);
  transition: transform var(--fd-duration-fast) var(--fd-easing-standard);
}

.advanced-panel--open .advanced-panel__chevron {
  transform: rotate(180deg);
}

.advanced-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: space-evenly;
  gap: var(--fd-space-12);
  padding: 10px 12px 14px;
  min-height: 0;
  overflow: auto;
}

/* Quality / max-edge: one full-width row each, outside the advanced panel.
   The rows share the leftover height with the advanced panel (both flex)
   so a tall card fills evenly instead of dumping all space below. */
.param-rows {
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: space-evenly;
  gap: var(--fd-space-8);
}

.slider-control {
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: center;
  gap: var(--fd-space-8);
  padding: 12px 14px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 14px;
  background: color-mix(in srgb, var(--fd-layer-1) 88%, transparent);
}

.slider-label {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--fd-space-4);
  font: var(--fd-text-caption);
  color: var(--fd-text-primary);
  font-weight: 500;
}

/* Numeric field beside the slider — same value, direct entry. */
.slider-field {
  display: inline-flex;
  align-items: center;
  gap: var(--fd-space-4);
  min-height: 28px;
  padding: 0 2px;
  border: 1px solid var(--fd-control-stroke);
  border-radius: 8px;
  background: var(--fd-layer-2);
}

.slider-field:focus-within {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.slider-field--invalid {
  border-color: var(--fd-danger-border);
}

.slider-field input {
  width: 52px;
  padding: 0 6px;
  border: none;
  background: transparent;
  color: var(--fd-text-primary);
  font: 600 14px/20px var(--fd-font-family);
  font-variant-numeric: tabular-nums;
  text-align: right;
  outline: none;
  appearance: textfield;
  -moz-appearance: textfield;
}

.slider-field input::-webkit-outer-spin-button,
.slider-field input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.slider-field input:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.slider-field__unit {
  padding-right: 6px;
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
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

.toggle-list {
  /* Natural-height rows, evenly distributed within the advanced panel —
     stretching the chips themselves reads as empty boxes. */
  display: grid;
  flex: 0 0 auto;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-10);
}

.toggle-chip {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  min-height: 52px;
  padding: 0 12px;
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

/* Tall windows: grow the card's controls so the stretched card doesn't swim
   in whitespace. */
@media (min-height: 900px) {
  .settings-dock {
    gap: var(--fd-space-12);
  }

  .preset-pill {
    min-height: 54px;
  }

  .slider-control {
    padding: 14px 16px;
  }

  .advanced-content {
    gap: var(--fd-space-16);
  }
}

@media (max-width: 768px) {
  .preset-ribbon {
    grid-template-columns: 1fr;
  }
}
</style>
