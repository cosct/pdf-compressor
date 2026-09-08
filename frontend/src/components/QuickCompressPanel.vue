<script setup lang="ts">
/**
 * Quick-mode (right-click) settings — edits the profile that the headless
 * `pdf-compressor-cli quick` path consumes, so file-manager compress actions
 * follow the choices made here without ever opening the main window.
 * 右键快速压缩设置 —— 编辑供 `pdf-compressor-cli quick` 后台模式消费的
 * 配置档案，文件管理器右键动作不打开主窗口也会遵循这里的参数。
 *
 * Mode selection mirrors the main view (presets + target size). Specific
 * parameters are NOT editable here: they always follow the preset profiles
 * from the compression settings panel — only the target size is set.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { loadPresetProfiles } from '../config/presets'
import type { QuickProfilePayload } from '../lib/bindings'
import { getQuickProfile, saveQuickProfile } from '../lib/tauri'
import type { PresetProfileMap } from '../types/pdf'
import {
  calculateMaxImageSizePx,
  DEFAULT_REFERENCE_IMAGE_EDGE_PX,
} from '../utils/compressionSettings'
import UnitSelect from './UnitSelect.vue'

defineProps<{ nativeAvailable: boolean }>()

const emit = defineEmits<{
  saved: []
  error: [error: unknown]
}>()

const { t } = useI18n()

type PresetMode = 'conservative' | 'balanced' | 'maximum' | 'custom'
type QuickMode = PresetMode | 'target'
const MODE_ORDER: QuickMode[] = ['conservative', 'balanced', 'maximum', 'custom', 'target']

/** Stored value stays in MB; the unit only shapes the input display. */
const TARGET_UNITS = ['KB', 'MB', 'GB'] as const

const mode = ref<QuickMode>('balanced')
/** Preset carried by the payload while target-size mode is active. */
const lastPresetMode = ref<PresetMode>('balanced')
const targetMb = ref(5)
const targetUnit = ref<string>('MB')
const targetInvalid = ref(false)

const loading = ref(true)
const saving = ref(false)
const presetProfiles = ref<PresetProfileMap | null>(null)

// Engine-side fallbacks per preset (pdf-core settings.rs) — used until the
// (possibly user-customized) preset profiles finish loading. Presets store a
// percentage; quick mode has no per-file analysis reference, so the px value
// is derived against the shared reference edge.
const ENGINE_FALLBACK = {
  conservative: { imageQuality: 82, maxImageSizePercent: 100 },
  balanced: { imageQuality: 72, maxImageSizePercent: 80 },
  maximum: { imageQuality: 58, maxImageSizePercent: 60 },
} as const

/**
 * Parameter set for a preset mode, always mirroring the compression preset
 * profiles (user-customized when saved in the settings panel).
 */
function presetParams(preset: PresetMode) {
  const profile = presetProfiles.value?.[preset]
  if (profile) {
    return {
      imageQuality: profile.imageQuality,
      maxImageSizePx: calculateMaxImageSizePx(
        profile.maxImageSizePercent,
        DEFAULT_REFERENCE_IMAGE_EDGE_PX,
      ),
      optimizeImages: profile.optimizeImages ?? true,
      compressStreams: profile.compressStreams ?? true,
      stripMetadata: profile.stripMetadata ?? true,
      grayscale: profile.grayscale ?? false,
      bilevelCodec: profile.bilevelCodec === 'ccitt-g4' ? ('ccitt-g4' as const) : ('jpeg' as const),
      subsetFonts: profile.subsetFonts ?? false,
      cmykConversion: profile.cmykConversion ?? true,
    }
  }
  const base = ENGINE_FALLBACK[preset === 'custom' ? 'maximum' : preset]
  return {
    imageQuality: base.imageQuality,
    maxImageSizePx: calculateMaxImageSizePx(
      base.maxImageSizePercent,
      DEFAULT_REFERENCE_IMAGE_EDGE_PX,
    ),
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
    grayscale: false,
    bilevelCodec: 'jpeg' as const,
    subsetFonts: preset === 'maximum' || preset === 'custom',
    cmykConversion: true,
  }
}

const modeOptions = computed(() =>
  MODE_ORDER.map((value) => ({
    value,
    title: value === 'target' ? t('settings.targetMode') : t(`app.preset.${value}`),
  })),
)

/** Preset whose parameters target-size mode rides on. */
const activePreset = computed<PresetMode>(() =>
  mode.value === 'target' ? lastPresetMode.value : mode.value,
)

const presetParamsView = computed(() => presetParams(activePreset.value))

/** Percent form of the size cap — px depends on a per-file reference the
 * quick path has no access to, so the display stays on the stored value. */
const presetPercentView = computed(() => {
  const preset = activePreset.value
  const profile = presetProfiles.value?.[preset]
  if (profile) {
    return profile.maxImageSizePercent
  }
  return ENGINE_FALLBACK[preset === 'custom' ? 'maximum' : preset].maxImageSizePercent
})

// --- Color mode (selectable override on top of the preset profile) --------

type ColorMode = 'color' | 'gray' | 'bw'

const colorMode = ref<ColorMode>('color')

function colorModeOf(grayscale: boolean, bilevelCodec: string): ColorMode {
  return bilevelCodec === 'ccitt-g4' ? 'bw' : grayscale ? 'gray' : 'color'
}

/** Reset the color-mode choice to whatever the preset profile carries. */
function syncColorModeFromPreset(preset: PresetMode) {
  const params = presetParams(preset)
  colorMode.value = colorModeOf(params.grayscale, params.bilevelCodec)
}

const colorModeOptions = computed(() => [
  { value: 'color' as const, title: t('settings.colorModeColor') },
  { value: 'gray' as const, title: t('settings.colorModeGray') },
  { value: 'bw' as const, title: t('settings.colorModeBw') },
])

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

const targetDisplayValue = computed(() => String(Math.round(mbToUnit(targetMb.value) * 100) / 100))

function commitTargetInput(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim()
  const parsed = Number(raw)
  const mb = Number.isFinite(parsed) ? unitToMb(parsed) : Number.NaN

  if (raw && Number.isFinite(mb) && mb > 0 && mb <= 2048) {
    targetInvalid.value = false
    targetMb.value = mb
    return
  }
  targetInvalid.value = true
}

function handleTargetInput() {
  if (targetInvalid.value) {
    targetInvalid.value = false
  }
}

// --- Profile persistence ----------------------------------------------------

function applyProfile(profile: QuickProfilePayload) {
  const stored = profile.preset
  const resolved: PresetMode =
    stored === 'conservative' || stored === 'maximum' || stored === 'custom' ? stored : 'balanced'
  lastPresetMode.value = resolved

  if (profile.targetSizeBytes != null) {
    mode.value = 'target'
    targetMb.value = Math.max(
      0.1,
      Math.min(2048, Math.round((profile.targetSizeBytes / (1024 * 1024)) * 100) / 100),
    )
  } else {
    mode.value = resolved
  }

  // A saved profile carries the color mode it was saved with; an empty
  // profile falls back to the preset profile's own conversion settings.
  if (profile.grayscale != null || profile.bilevelCodec != null) {
    colorMode.value = colorModeOf(profile.grayscale ?? false, profile.bilevelCodec ?? 'jpeg')
  } else {
    syncColorModeFromPreset(resolved)
  }
}

async function load() {
  loading.value = true
  try {
    const [profile, profiles] = await Promise.all([getQuickProfile(), loadPresetProfiles()])
    presetProfiles.value = profiles
    applyProfile(profile)
  } catch (error) {
    emit('error', error)
  } finally {
    loading.value = false
  }
}

function selectMode(next: QuickMode) {
  if (next === 'target') {
    mode.value = 'target'
    return
  }
  mode.value = next
  lastPresetMode.value = next
  // Switching presets resets the color-mode override to the preset's own.
  syncColorModeFromPreset(next)
}

function buildPayload(): QuickProfilePayload {
  const preset = mode.value === 'target' ? lastPresetMode.value : mode.value
  return {
    // v2: cmykConversion opt-outs are genuine from this version on (v1
    // values get the default-flip migration on load).
    version: 2,
    preset,
    ...presetParams(preset),
    grayscale: colorMode.value !== 'color',
    bilevelCodec: colorMode.value === 'bw' ? 'ccitt-g4' : 'jpeg',
    targetSizeBytes: mode.value === 'target' ? Math.round(targetMb.value * 1024 * 1024) : null,
  }
}

const canSave = computed(() => !loading.value && !saving.value && !targetInvalid.value)

async function save() {
  if (!canSave.value) {
    return
  }
  saving.value = true
  try {
    await saveQuickProfile(buildPayload())
    emit('saved')
  } catch (error) {
    emit('error', error)
  } finally {
    saving.value = false
  }
}

async function reset() {
  saving.value = true
  try {
    // An all-empty profile makes the CLI fall back to the engine defaults.
    await saveQuickProfile({ version: 2 })
    applyProfile({ version: 2 })
    emit('saved')
  } catch (error) {
    emit('error', error)
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="quick-panel" :aria-busy="loading ? 'true' : 'false'">
    <header class="panel-header">
      <span class="panel-header__icon" aria-hidden="true">
        <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
          <path
            d="M10.5 2 4.8 9.2h3.4L8 16l5.7-7.2h-3.4L10.5 2Z"
            stroke="var(--fd-accent)"
            stroke-width="1.4"
            stroke-linejoin="round"
          />
        </svg>
      </span>
      <h2>{{ t('quick.title') }}</h2>
    </header>

    <div class="quick-panel__body">
      <div class="quick-field">
        <span class="quick-field__label">{{ t('settings.compressionMode') }}</span>
        <div class="quick-modes" role="radiogroup" :aria-label="t('settings.compressionMode')">
          <button
            v-for="option in modeOptions"
            :key="option.value"
            type="button"
            role="radio"
            :aria-checked="mode === option.value ? 'true' : 'false'"
            class="quick-modes__item"
            :class="{ 'quick-modes__item--active': mode === option.value }"
            :disabled="saving"
            @click="selectMode(option.value)"
          >
            {{ option.title }}
          </button>
        </div>
      </div>

      <!-- Read-only echo of the preset profile's parameters: switching modes
           reflects the current values without allowing edits here. Target
           size mode hides it — the engine searches parameters itself to fit
           the budget. -->
      <div v-if="mode !== 'target'" class="quick-field">
        <span class="quick-field__label">{{ t('quick.params') }}</span>
        <div class="quick-params" :aria-label="t('quick.params')">
          <div class="quick-param">
            <span class="quick-param__label">{{ t('settings.quality') }}</span>
            <span class="quick-param__value">{{ presetParamsView.imageQuality }}</span>
          </div>
          <div class="quick-param">
            <span class="quick-param__label">{{ t('settings.maxEdge') }}</span>
            <span class="quick-param__value">{{ presetPercentView }}%</span>
          </div>
        </div>
      </div>

      <div v-if="mode === 'target'" class="quick-field">
        <span class="quick-field__label">{{ t('settings.targetMode') }}</span>
        <div class="quick-target-entry" :class="{ 'quick-target-entry--invalid': targetInvalid }">
          <input
            type="text"
            inputmode="decimal"
            :value="targetDisplayValue"
            :disabled="saving"
            :aria-label="t('settings.targetMode')"
            :aria-invalid="targetInvalid ? 'true' : 'false'"
            @input="handleTargetInput"
            @change="commitTargetInput"
            @keydown.enter.prevent="commitTargetInput"
          />
          <UnitSelect
            v-model="targetUnit"
            :options="TARGET_UNITS"
            :disabled="saving"
            :aria-label="t('settings.targetSizeUnit')"
          />
        </div>
        <p v-if="targetInvalid" class="quick-field__error" role="alert">
          {{ t('settings.targetSizeInvalid') }}
        </p>
      </div>

      <div class="quick-field">
        <span class="quick-field__label">{{ t('settings.colorMode') }}</span>
        <div class="quick-modes" role="radiogroup" :aria-label="t('settings.colorMode')">
          <button
            v-for="option in colorModeOptions"
            :key="option.value"
            type="button"
            role="radio"
            :aria-checked="colorMode === option.value ? 'true' : 'false'"
            class="quick-modes__item"
            :class="{ 'quick-modes__item--active': colorMode === option.value }"
            :disabled="saving"
            @click="colorMode = option.value"
          >
            {{ option.title }}
          </button>
        </div>
      </div>

      <p class="quick-panel__hint">{{ t('quick.followsPresets') }}</p>
    </div>

    <footer class="quick-panel__footer">
      <p class="quick-panel__note">{{ t('quick.note') }}</p>
      <div class="quick-panel__actions">
        <button type="button" class="fd-button fd-button--subtle" :disabled="saving" @click="reset">
          {{ t('settings.resetPresets') }}
        </button>
        <button
          type="button"
          class="fd-button fd-button--accent"
          :disabled="!canSave"
          @click="save"
        >
          {{ saving ? t('quick.saving') : t('quick.save') }}
        </button>
      </div>
    </footer>
  </section>
</template>

<style scoped>
.quick-panel {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-16);
}

.quick-panel__body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: var(--fd-space-16);
}

.quick-field {
  /* Each field band grows and centers its control, so a stretched card
     fills evenly instead of clustering at the top with dead space below. */
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: center;
  gap: var(--fd-space-8);
  min-width: 0;
}

.quick-field__label {
  color: var(--fd-text-secondary);
  font: var(--fd-text-body-strong);
}

.quick-field__error {
  margin: 0;
  color: var(--fd-danger);
  font: var(--fd-text-caption);
}

/* Read-only echo of the preset profile parameters. */
.quick-params {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-8);
}

.quick-param {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-8);
  min-height: var(--fd-control-height-md);
  padding: 0 var(--fd-space-12);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: color-mix(in srgb, var(--fd-layer-1) 70%, transparent);
  color: var(--fd-text-secondary);
}

.quick-param__label {
  font: var(--fd-text-caption);
}

.quick-param__value {
  font: 600 15px/20px var(--fd-font-family);
  font-variant-numeric: tabular-nums;
  color: var(--fd-text-primary);
}

.quick-modes {
  display: flex;
  gap: var(--fd-space-4);
  padding: var(--fd-space-4);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-layer-1);
}

.quick-modes__item {
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

.quick-modes__item:hover:not(:disabled) {
  background: var(--fd-subtle-bg-hover);
}

.quick-modes__item--active {
  background: var(--fd-surface-raised);
  border-color: var(--fd-stroke-card);
  color: var(--fd-text-primary);
  box-shadow: var(--fd-shadow-4);
}

.quick-modes__item:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.quick-modes__item:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.quick-target-entry {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
  border: 1px solid var(--fd-control-stroke);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-control-bg);
}

.quick-target-entry:focus-within {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.quick-target-entry--invalid {
  border-color: var(--fd-danger-border);
}

.quick-target-entry input {
  flex: 1;
  min-width: 0;
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-8);
  border: none;
  background: transparent;
  outline: none;
  font: var(--fd-text-body);
  font-variant-numeric: tabular-nums;
}

.quick-panel__hint {
  margin: 0;
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  line-height: 1.5;
}

.quick-panel__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-16);
  flex-wrap: wrap;
  margin-top: auto;
}

.quick-panel__note {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  flex: 1;
  min-width: 220px;
}

.quick-panel__actions {
  display: flex;
  gap: var(--fd-space-10);
}

.quick-panel__actions .fd-button {
  min-height: 36px;
}

/* Tall windows: grow the fields so the stretched card stays balanced. */
@media (min-height: 900px) {
  .quick-panel {
    gap: var(--fd-space-20);
  }

  .quick-modes__item {
    min-height: 40px;
    font: var(--fd-text-body);
  }

  .quick-param {
    min-height: 48px;
  }

  .quick-param__value {
    font-size: 16px;
  }

  .quick-target-entry input {
    min-height: 40px;
  }

  .quick-panel__actions .fd-button {
    min-height: 40px;
  }
}
</style>
