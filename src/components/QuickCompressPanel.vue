<script setup lang="ts">
/**
 * Quick-mode (right-click) settings — edits the profile that the headless
 * `pdf-compressor-cli quick` path consumes, so file-manager compress actions
 * follow the choices made here without ever opening the main window.
 * 右键快速压缩设置 —— 编辑供 `pdf-compressor-cli quick` 后台模式消费的
 * 配置档案，文件管理器右键动作不打开主窗口也会遵循这里的参数。
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { QuickProfilePayload } from '../lib/bindings'
import { getQuickProfile, saveQuickProfile } from '../lib/tauri'

defineProps<{ nativeAvailable: boolean }>()

const emit = defineEmits<{
  saved: []
  error: [error: unknown]
}>()

const { t } = useI18n()

// Mirrors the engine's per-preset defaults (pdf-core settings.rs) — the
// quick profile stores absolute pixels because no per-file analysis
// reference edge exists in the headless path.
const PRESET_DEFAULTS = {
  conservative: { quality: 82, maxEdge: 2400 },
  balanced: { quality: 72, maxEdge: 1800 },
  maximum: { quality: 58, maxEdge: 1400 },
} as const
type QuickPreset = keyof typeof PRESET_DEFAULTS
const PRESET_ORDER: QuickPreset[] = ['conservative', 'balanced', 'maximum']

type ColorMode = 'color' | 'grayscale' | 'g4'

const preset = ref<QuickPreset>('balanced')
const colorMode = ref<ColorMode>('color')
const quality = ref<number>(PRESET_DEFAULTS.balanced.quality)
const maxEdge = ref<number>(PRESET_DEFAULTS.balanced.maxEdge)
const targetSizeMb = ref('')
const optimizeImages = ref(true)
const compressStreams = ref(true)
const stripMetadata = ref(true)
const subsetFonts = ref(false)

const loading = ref(true)
const saving = ref(false)

const targetSizeInvalid = computed(() => {
  const raw = targetSizeMb.value.trim()
  if (!raw) {
    return false
  }
  const value = Number(raw)
  return !Number.isFinite(value) || value < 0.1 || value > 2048
})

const canSave = computed(
  () => !loading.value && !saving.value && !targetSizeInvalid.value,
)

function applyProfile(profile: QuickProfilePayload) {
  const resolvedPreset =
    profile.preset === 'conservative' || profile.preset === 'maximum'
      ? profile.preset
      : 'balanced'
  preset.value = resolvedPreset
  quality.value = profile.imageQuality ?? PRESET_DEFAULTS[resolvedPreset].quality
  maxEdge.value = profile.maxImageSizePx ?? PRESET_DEFAULTS[resolvedPreset].maxEdge
  colorMode.value = profile.bilevelCodec === 'ccitt-g4' ? 'g4' : profile.grayscale ? 'grayscale' : 'color'
  targetSizeMb.value =
    profile.targetSizeBytes != null
      ? String(Math.round((profile.targetSizeBytes / (1024 * 1024)) * 100) / 100)
      : ''
  optimizeImages.value = profile.optimizeImages ?? true
  compressStreams.value = profile.compressStreams ?? true
  stripMetadata.value = profile.stripMetadata ?? true
  subsetFonts.value = profile.subsetFonts ?? false
}

async function load() {
  loading.value = true
  try {
    applyProfile(await getQuickProfile())
  } catch (error) {
    emit('error', error)
  } finally {
    loading.value = false
  }
}

function choosePreset(next: QuickPreset) {
  preset.value = next
  quality.value = PRESET_DEFAULTS[next].quality
  maxEdge.value = PRESET_DEFAULTS[next].maxEdge
}

function buildPayload(): QuickProfilePayload {
  const trimmed = targetSizeMb.value.trim()
  return {
    version: 1,
    preset: preset.value,
    imageQuality: Math.round(quality.value),
    maxImageSizePx: Math.round(maxEdge.value),
    optimizeImages: optimizeImages.value,
    compressStreams: compressStreams.value,
    stripMetadata: stripMetadata.value,
    grayscale: colorMode.value !== 'color',
    bilevelCodec: colorMode.value === 'g4' ? 'ccitt-g4' : 'jpeg',
    subsetFonts: subsetFonts.value,
    targetSizeBytes: trimmed ? Math.round(Number(trimmed) * 1024 * 1024) : null,
  }
}

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
    await saveQuickProfile({ version: 1 })
    applyProfile({ version: 1 })
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
          <path d="M3 5.5h9M14.5 5.5H15M3 12.5h3M8.5 12.5H15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
          <circle cx="13" cy="5.5" r="1.8" stroke="currentColor" stroke-width="1.4"/>
          <circle cx="7" cy="12.5" r="1.8" stroke="currentColor" stroke-width="1.4"/>
        </svg>
      </span>
      <div class="panel-header__text">
        <h2>{{ t('quick.title') }}</h2>
        <p class="panel-header__subtitle">{{ t('quick.subtitle') }}</p>
      </div>
    </header>

    <div class="quick-panel__body">
      <div class="quick-field">
        <span class="quick-field__label">{{ t('quick.preset') }}</span>
        <div class="quick-segmented" role="radiogroup" :aria-label="t('quick.preset')">
          <button
            v-for="option in PRESET_ORDER"
            :key="option"
            type="button"
            role="radio"
            :aria-checked="preset === option ? 'true' : 'false'"
            class="quick-segmented__option"
            :class="{ 'quick-segmented__option--active': preset === option }"
            @click="choosePreset(option)"
          >
            {{ t(`app.preset.${option}`) }}
          </button>
        </div>
      </div>

      <div class="quick-field">
        <span class="quick-field__label">{{ t('settings.colorMode') }}</span>
        <div class="quick-segmented" role="radiogroup" :aria-label="t('settings.colorMode')">
          <button
            v-for="option in (['color', 'grayscale', 'g4'] as ColorMode[])"
            :key="option"
            type="button"
            role="radio"
            :aria-checked="colorMode === option ? 'true' : 'false'"
            class="quick-segmented__option"
            :class="{ 'quick-segmented__option--active': colorMode === option }"
            @click="colorMode = option"
          >
            {{ option === 'color' ? t('settings.colorModeColor') : option === 'grayscale' ? t('settings.colorModeGray') : t('settings.colorModeBw') }}
          </button>
        </div>
      </div>

      <div class="quick-field quick-field--inline">
        <label class="quick-field__label" for="quick-quality">
          {{ t('settings.quality') }}
          <span class="quick-field__value">{{ Math.round(quality) }}</span>
        </label>
        <input
          id="quick-quality"
          v-model.number="quality"
          class="quick-slider"
          type="range"
          min="10"
          max="100"
          step="1"
        />
      </div>

      <div class="quick-field-row">
        <div class="quick-field">
          <label class="quick-field__label" for="quick-max-edge">{{ t('settings.maxEdge') }}</label>
          <div class="quick-input">
            <input
              id="quick-max-edge"
              v-model.number="maxEdge"
              type="number"
              min="100"
              max="8000"
              step="50"
            />
            <span class="quick-input__unit">{{ t('quick.maxEdgeUnit') }}</span>
          </div>
        </div>

        <div class="quick-field">
          <label class="quick-field__label" for="quick-target-size">{{ t('quick.targetSize') }}</label>
          <div class="quick-input" :class="{ 'quick-input--invalid': targetSizeInvalid }">
            <input
              id="quick-target-size"
              v-model="targetSizeMb"
              type="text"
              inputmode="decimal"
              :placeholder="t('settings.targetSizeOff')"
              :aria-invalid="targetSizeInvalid ? 'true' : 'false'"
            />
            <span class="quick-input__unit">MB</span>
          </div>
          <p v-if="targetSizeInvalid" class="quick-field__error">{{ t('settings.targetSizeInvalid') }}</p>
        </div>
      </div>

      <div class="quick-toggles">
        <label class="quick-toggle">
          <span class="fd-toggle"><input v-model="optimizeImages" type="checkbox" /></span>
          <span>{{ t('settings.optimizeImages') }}</span>
        </label>
        <label class="quick-toggle">
          <span class="fd-toggle"><input v-model="compressStreams" type="checkbox" /></span>
          <span>{{ t('settings.compressStreams') }}</span>
        </label>
        <label class="quick-toggle">
          <span class="fd-toggle"><input v-model="stripMetadata" type="checkbox" /></span>
          <span>{{ t('settings.stripMetadata') }}</span>
        </label>
        <label class="quick-toggle">
          <span class="fd-toggle"><input v-model="subsetFonts" type="checkbox" /></span>
          <span>{{ t('settings.subsetFonts') }}</span>
        </label>
      </div>
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
  gap: var(--fd-space-20);
}

.panel-header__text {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-2);
  min-width: 0;
}

.panel-header__subtitle {
  color: var(--fd-text-secondary);
  font: var(--fd-text-caption);
}

.quick-panel__body {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-16);
}

.quick-field {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-8);
  min-width: 0;
}

.quick-field-row {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-16);
}

.quick-field__label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: var(--fd-text-secondary);
  font: var(--fd-text-body-strong);
}

.quick-field__value {
  color: var(--fd-accent);
  font-variant-numeric: tabular-nums;
}

.quick-field__error {
  color: var(--fd-danger);
  font: var(--fd-text-caption);
}

.quick-segmented {
  display: flex;
  gap: var(--fd-space-4);
  padding: var(--fd-space-4);
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-layer-1);
}

.quick-segmented__option {
  flex: 1;
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-12);
  border: 1px solid transparent;
  border-radius: calc(var(--fd-radius-sm) - 4px);
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard);
}

.quick-segmented__option:hover {
  background: var(--fd-subtle-bg-hover);
}

.quick-segmented__option--active {
  background: var(--fd-surface-raised);
  border-color: var(--fd-stroke-card);
  color: var(--fd-text-primary);
  box-shadow: var(--fd-shadow-4);
}

.quick-segmented__option:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.quick-slider {
  width: 100%;
  accent-color: var(--fd-accent);
}

.quick-input {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  border: 1px solid var(--fd-control-stroke);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-control-bg);
  padding: 0 var(--fd-space-12);
  min-height: var(--fd-control-height-md);
}

.quick-input:focus-within {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.quick-input--invalid {
  border-color: var(--fd-danger-border);
}

.quick-input input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-variant-numeric: tabular-nums;
}

.quick-input__unit {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
}

.quick-toggles {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-10) var(--fd-space-16);
}

.quick-toggle {
  display: flex;
  align-items: center;
  gap: var(--fd-space-10);
  color: var(--fd-text-primary);
  cursor: pointer;
}

.quick-panel__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-16);
  flex-wrap: wrap;
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

@media (max-width: 640px) {
  .quick-field-row,
  .quick-toggles {
    grid-template-columns: 1fr;
  }
}
</style>
