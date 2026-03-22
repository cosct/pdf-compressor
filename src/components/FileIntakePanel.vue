<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { listenForNativePdfDrop, type NativePdfDropEvent } from '../lib/tauri'

type FileWithPath = File & {
  path?: string
}

const props = defineProps<{
  modelValue: string
  busy: boolean
  nativeAvailable: boolean
  pathTone: 'neutral' | 'warning' | 'success'
  pathMessage: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  browse: []
  analyze: []
  clear: []
}>()

const { t } = useI18n()

const dragActive = ref(false)
const dropFeedback = ref('')
let stopListening: (() => void) | null = null

const pathInputId = 'pdf-source-path'
const pathHintId = 'pdf-source-path-hint'
const pathStateId = 'pdf-source-path-state'

const fileName = computed(() => {
  const trimmed = props.modelValue.trim()
  if (!trimmed) {
    return ''
  }

  const segments = trimmed.split(/[/\\]/)
  return segments[segments.length - 1] || trimmed
})

const readyToAnalyze = computed(() => props.modelValue.trim().toLowerCase().endsWith('.pdf'))
const browseLabel = computed(() =>
  props.nativeAvailable ? t('intake.browse') : t('intake.browseDisabledPreview'),
)

function isPdfPath(path: string): boolean {
  return path.trim().toLowerCase().endsWith('.pdf')
}

function normalizeDroppedValue(rawValue: string): string {
  const firstLine = rawValue.trim().split(/\r?\n/)[0] ?? ''
  let normalized = firstLine.replace(/^file:\/\//i, '')

  if (/^\/[A-Za-z]:\//.test(normalized)) {
    normalized = normalized.slice(1)
  }

  try {
    return decodeURIComponent(normalized)
  } catch {
    return normalized
  }
}

function applyDroppedPath(path: string | null) {
  if (!path || !isPdfPath(path)) {
    dropFeedback.value = t('intake.invalidDrop')
    return
  }

  dropFeedback.value = ''
  emit('update:modelValue', path)
  emit('analyze')
}

function readDroppedPath(event: DragEvent): string | null {
  const files = Array.from(event.dataTransfer?.files ?? []) as FileWithPath[]
  const directPath = files.find(
    (file) => file.name.toLowerCase().endsWith('.pdf') && typeof file.path === 'string' && file.path.trim(),
  )?.path

  if (directPath) {
    return directPath
  }

  const candidates = [
    event.dataTransfer?.getData('text/plain') ?? '',
    event.dataTransfer?.getData('text/uri-list') ?? '',
  ]

  for (const candidate of candidates) {
    const normalized = normalizeDroppedValue(candidate)
    if (isPdfPath(normalized)) {
      return normalized
    }
  }

  return null
}

function handleNativeDropEvent(event: NativePdfDropEvent) {
  switch (event.type) {
    case 'enter':
    case 'over':
      dragActive.value = true
      break
    case 'leave':
      dragActive.value = false
      break
    case 'drop':
      dragActive.value = false
      applyDroppedPath(event.paths.find((path) => isPdfPath(path)) ?? null)
      break
  }
}

function handleDragOver() {
  dragActive.value = true
}

function handleDragLeave() {
  dragActive.value = false
}

function handleDrop(event: DragEvent) {
  dragActive.value = false
  applyDroppedPath(readDroppedPath(event))
}

onMounted(async () => {
  stopListening = await listenForNativePdfDrop(handleNativeDropEvent)
})

onBeforeUnmount(() => {
  stopListening?.()
})
</script>

<template>
  <section class="panel-surface intake-panel">
    <div class="section-header intake-panel__header">
      <div>
        <p class="section-kicker">{{ t('intake.eyebrow') }}</p>
        <h2>{{ t('intake.title') }}</h2>
        <p class="intake-panel__body">{{ t('intake.body') }}</p>
      </div>
      <span class="app-chip app-chip--accent">{{ t('intake.dropTag') }}</span>
    </div>

    <div
      class="dropzone"
      :class="{ 'dropzone--active': dragActive, 'dropzone--filled': fileName }"
      :aria-busy="props.busy ? 'true' : 'false'"
      @dragenter.prevent="handleDragOver"
      @dragover.prevent="handleDragOver"
      @dragleave.prevent="handleDragLeave"
      @drop.prevent="handleDrop"
    >
      <div class="dropzone__halo" aria-hidden="true"></div>
      <span class="dropzone__pill">{{ fileName ? t('intake.dropReady') : t('intake.dropIdle') }}</span>

      <div class="dropzone__headline">
        <strong class="dropzone__filename" :title="fileName || t('intake.emptySelection')">{{ fileName || t('intake.emptySelection') }}</strong>
        <p>{{ t('intake.dropCaption') }}</p>
      </div>

      <div class="dropzone__actions">
        <button
          class="app-button app-button--primary dropzone__browse"
          type="button"
          :disabled="props.busy || !props.nativeAvailable"
          :aria-disabled="props.busy || !props.nativeAvailable ? 'true' : 'false'"
          @click.stop="emit('browse')"
        >
          {{ browseLabel }}
        </button>
        <div class="dropzone__supporting-actions">
          <button class="app-button" type="button" :disabled="props.busy || !readyToAnalyze" @click.stop="emit('analyze')">
            {{ t('intake.analyze') }}
          </button>
          <button class="app-button app-button--ghost" type="button" :disabled="props.busy || !props.modelValue.trim()" @click.stop="emit('clear')">
            {{ t('intake.clear') }}
          </button>
        </div>
        <p v-if="!props.nativeAvailable" class="dropzone__browse-note">{{ t('intake.pathHintManual') }}</p>
      </div>

      <ul class="dropzone__benefits">
        <li>{{ t('intake.benefits.instant') }}</li>
        <li>{{ t('intake.benefits.guided') }}</li>
        <li>{{ t('intake.benefits.local') }}</li>
      </ul>
    </div>

    <div v-if="dropFeedback" class="path-state" data-tone="warning" role="alert">
      <strong>{{ t('app.alertTitle') }}</strong>
      <span>{{ dropFeedback }}</span>
    </div>

    <div class="intake-panel__support">
      <label class="path-field">
        <span class="path-field__label">{{ t('intake.pathLabel') }}</span>
        <input
          :id="pathInputId"
          :value="props.modelValue"
          class="app-input"
          type="text"
          :placeholder="t('intake.pathPlaceholder')"
          :disabled="props.busy"
          :aria-describedby="`${pathHintId} ${pathStateId}`"
          :aria-invalid="props.pathTone === 'warning' ? 'true' : 'false'"
          @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        />
        <span :id="pathHintId" class="path-field__hint">
          {{ props.nativeAvailable ? t('intake.pathHintNative') : t('intake.pathHintManual') }}
        </span>
      </label>

      <div
        :id="pathStateId"
        class="path-state"
        :data-tone="props.pathTone"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <strong>{{ t('intake.pathStatus') }}</strong>
        <span>{{ props.pathMessage }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.intake-panel,
.path-field,
.intake-panel__support,
.dropzone__actions {
  display: grid;
  gap: var(--space-4);
}

.intake-panel {
  min-height: 100%;
}

.intake-panel__body,
.path-field__hint,
.path-state span,
.dropzone__headline p,
.dropzone__browse-note {
  margin: 0;
  color: var(--color-ink-muted);
}

.dropzone,
.path-field,
.path-state {
  border: 1px solid var(--color-line);
  border-radius: var(--radius-3);
  background: var(--color-surface-strong);
}

.dropzone {
  position: relative;
  display: grid;
  gap: var(--space-5);
  min-height: 24rem;
  padding: var(--space-5);
  align-content: start;
  justify-items: stretch;
  border-style: dashed;
  border-color: var(--color-line-strong);
  background:
    radial-gradient(circle at top right, rgba(255, 157, 87, 0.18), transparent 26%),
    radial-gradient(circle at bottom left, rgba(120, 211, 203, 0.16), transparent 24%),
    linear-gradient(180deg, rgba(10, 22, 34, 0.92), rgba(18, 36, 54, 0.96));
  transition:
    border-color var(--transition-fast),
    transform var(--transition-fast),
    box-shadow var(--transition-fast),
    background-color var(--transition-fast);
}

.dropzone--active {
  border-color: var(--color-accent-strong);
  transform: translateY(-1px);
  box-shadow: 0 1.4rem 3.2rem rgba(4, 10, 19, 0.45);
}

.dropzone--filled {
  border-style: solid;
}

.dropzone__halo {
  position: absolute;
  inset: auto -4rem -4rem auto;
  width: 11rem;
  height: 11rem;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(255, 157, 87, 0.24), transparent 68%);
}

.dropzone__pill {
  position: relative;
  display: inline-flex;
  align-items: center;
  min-height: 2rem;
  justify-self: start;
  padding: 0 var(--space-3);
  border-radius: var(--radius-pill);
  background: rgba(255, 157, 87, 0.12);
  color: var(--color-accent-strong);
  font-size: 0.8125rem;
  font-weight: 600;
}

.dropzone__headline {
  position: relative;
  display: grid;
  gap: var(--space-2);
  max-width: 32rem;
}

.dropzone__filename {
  display: block;
  max-width: 100%;
  overflow-wrap: anywhere;
}

.dropzone strong {
  font-family: var(--font-display);
  font-size: clamp(2.2rem, 4vw, 3.7rem);
  line-height: 0.94;
  letter-spacing: -0.04em;
}

.dropzone__headline p {
  max-width: 38ch;
}

.dropzone__actions {
  position: relative;
  width: min(100%, 27rem);
  gap: var(--space-3);
}

.dropzone__browse {
  width: 100%;
  min-height: 3.75rem;
  font-size: var(--font-size-3);
}

.dropzone__supporting-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.dropzone__supporting-actions > .app-button {
  flex: 1 1 12rem;
}

.dropzone__browse-note {
  font-size: var(--font-size-1);
}

.dropzone__benefits {
  position: relative;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
  width: 100%;
  margin: 0;
  padding: 0;
  list-style: none;
}

.dropzone__benefits li {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-2);
  border: 1px solid var(--color-line);
  background: rgba(7, 17, 26, 0.44);
  color: var(--color-ink);
  font-size: 0.8125rem;
  line-height: 1.45;
}

.intake-panel__support {
  grid-template-columns: minmax(0, 1.35fr) minmax(15rem, 0.65fr);
  align-items: start;
  gap: var(--space-3);
}

.path-field {
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
}

.path-field__label,
.path-state strong {
  color: var(--color-ink-strong);
  font-weight: 600;
}

.path-state {
  display: grid;
  gap: var(--space-1);
  min-height: 100%;
  padding: var(--space-3);
}

.path-state[data-tone='warning'] {
  background: var(--color-warning-soft);
}

.path-state[data-tone='success'] {
  background: var(--color-success-soft);
}

@media (max-width: 62rem) {
  .dropzone__benefits,
  .intake-panel__support {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }
}

@media (max-width: 48rem) {
  .dropzone {
    min-height: 18rem;
    padding: var(--space-4);
  }

  .dropzone__actions,
  .dropzone__supporting-actions {
    width: 100%;
  }
}
</style>
