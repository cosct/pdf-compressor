<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { listenForNativePdfDrop, type NativePdfDropEvent } from '../lib/tauri'

type FileWithPath = File & {
  path?: string
}

type QueueItemStatus = 'selected' | 'analyzing' | 'ready' | 'compressing' | 'success' | 'error'

export interface QueueVisualItem {
  id: string
  fileName: string
  path: string
  status: QueueItemStatus
  presetLabel: string
  detail: string
  meta: string[]
  progressPercent: number
  overrideLabel?: string
  outputPath?: string | null
}

const props = withDefaults(
  defineProps<{
    items: QueueVisualItem[]
    selectedId?: string | null
    nativeAvailable: boolean
    queueLocked: boolean
  }>(),
  {
    selectedId: null,
  },
)

const emit = defineEmits<{
  'add-paths': [paths: string[]]
  browse: []
  select: [id: string]
  delete: [id: string]
  'open-result': [id: string]
  'open-result-folder': [id: string]
}>()

const { t } = useI18n()

const dragActive = ref(false)
const dropFeedback = ref('')
const contextMenu = ref<{ x: number; y: number; itemId: string } | null>(null)
let stopListening: (() => void) | null = null

const browseLabel = computed(() =>
  props.nativeAvailable ? t('intake.browse') : t('intake.browseDisabledPreview'),
)

const dropHintCopy = computed(() =>
  props.items.length ? t('upload.dropHintReady') : t('upload.dropHint'),
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

function applyDroppedPaths(paths: string[]) {
  const pdfPaths = paths.filter((path) => isPdfPath(path))

  if (!pdfPaths.length) {
    dropFeedback.value = t('intake.invalidDrop')
    return
  }

  dropFeedback.value = ''
  emit('add-paths', pdfPaths)
}

function readDroppedPaths(event: DragEvent): string[] {
  const files = Array.from(event.dataTransfer?.files ?? []) as FileWithPath[]
  const directPaths = files
    .filter(
      (file) => file.name.toLowerCase().endsWith('.pdf') && typeof file.path === 'string' && file.path.trim(),
    )
    .map((file) => file.path as string)

  if (directPaths.length) {
    return directPaths
  }

  const candidates = [
    event.dataTransfer?.getData('text/plain') ?? '',
    event.dataTransfer?.getData('text/uri-list') ?? '',
  ]

  const resolved: string[] = []

  for (const candidate of candidates) {
    for (const part of candidate.split(/\r?\n/)) {
      const normalized = normalizeDroppedValue(part)
      if (isPdfPath(normalized)) {
        resolved.push(normalized)
      }
    }
  }

  return resolved
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
      applyDroppedPaths(event.paths)
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
  applyDroppedPaths(readDroppedPaths(event))
}

function handleQueueItemKeydown(event: KeyboardEvent, id: string) {
  if (event.key !== 'Enter' && event.key !== ' ') {
    return
  }

  event.preventDefault()
  emit('select', id)
}

function closeContextMenu() {
  contextMenu.value = null
}

function handleGlobalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closeContextMenu()
  }
}

function handleQueueItemContextMenu(event: MouseEvent, item: QueueVisualItem) {
  emit('select', item.id)

  if (!item.outputPath) {
    closeContextMenu()
    return
  }

  contextMenu.value = {
    x: event.clientX,
    y: event.clientY,
    itemId: item.id,
  }
}

function openQueueItemResult() {
  if (!contextMenu.value) {
    return
  }

  emit('open-result', contextMenu.value.itemId)
  closeContextMenu()
}

function openQueueItemResultFolder() {
  if (!contextMenu.value) {
    return
  }

  emit('open-result-folder', contextMenu.value.itemId)
  closeContextMenu()
}

function statusTone(status: QueueItemStatus) {
  switch (status) {
    case 'ready':
    case 'success':
      return 'success'
    case 'error':
      return 'danger'
    case 'compressing':
    case 'analyzing':
      return 'accent'
    default:
      return 'neutral'
  }
}

function statusLabel(status: QueueItemStatus) {
  return t(`queue.status.${status}`)
}

onMounted(async () => {
  stopListening = await listenForNativePdfDrop(handleNativeDropEvent)
  window.addEventListener('click', closeContextMenu)
  window.addEventListener('blur', closeContextMenu)
  window.addEventListener('keydown', handleGlobalKeydown)
  window.addEventListener('resize', closeContextMenu)
  window.addEventListener('scroll', closeContextMenu, true)
})

onBeforeUnmount(() => {
  stopListening?.()
  window.removeEventListener('click', closeContextMenu)
  window.removeEventListener('blur', closeContextMenu)
  window.removeEventListener('keydown', handleGlobalKeydown)
  window.removeEventListener('resize', closeContextMenu)
  window.removeEventListener('scroll', closeContextMenu, true)
})
</script>

<template>
  <section
    class="upload-panel fd-card"
    :class="{ 'upload-panel--active': dragActive }"
    @dragenter.prevent="handleDragOver"
    @dragover.prevent="handleDragOver"
    @dragleave.prevent="handleDragLeave"
    @drop.prevent="handleDrop"
  >
    <div class="panel-header">
      <div class="panel-header__left">
        <svg class="panel-header__icon" width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true">
          <path d="M6 3a3 3 0 0 0-3 3v8a3 3 0 0 0 3 3h8a3 3 0 0 0 3-3V6a3 3 0 0 0-3-3H6Zm4 3.5a.5.5 0 0 1 .5.5v2.5H13a.5.5 0 0 1 0 1h-2.5V13a.5.5 0 0 1-1 0v-2.5H7a.5.5 0 0 1 0-1h2.5V7a.5.5 0 0 1 .5-.5Z" fill="var(--fd-accent)"/>
        </svg>
        <h2>{{ t('upload.eyebrow') }}</h2>
      </div>
      <div class="panel-header__right">
        <span v-if="props.items.length" class="fd-badge">{{ t('queue.count', { count: props.items.length }) }}</span>
        <span v-if="props.queueLocked" class="fd-badge fd-badge--accent">{{ t('upload.lockedTag') }}</span>
      </div>
    </div>

    <div class="upload-stage" :class="{ 'upload-stage--filled': props.items.length }">
      <div v-if="!props.items.length" class="dropzone" :class="{ 'dropzone--active': dragActive }">
        <div class="dropzone__inner">
          <div class="dropzone__icon" aria-hidden="true">
            <svg width="48" height="48" viewBox="0 0 48 48" fill="none">
              <rect x="8" y="6" width="32" height="36" rx="4" stroke="currentColor" stroke-width="1.5" fill="none" opacity="0.45"/>
              <path d="M24 32V18m0 0l-5 5m5-5l5 5" stroke="var(--fd-accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </div>
          <div class="dropzone__text">
            <strong>{{ t('upload.dropTitle') }}</strong>
            <p>{{ dropHintCopy }}</p>
          </div>
          <div class="dropzone__actions">
            <button
              class="fd-button fd-button--accent"
              type="button"
              :disabled="!props.nativeAvailable"
              @click.stop="emit('browse')"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.879a1.5 1.5 0 0 1 1.06.44L8.56 4.56A1.5 1.5 0 0 0 9.621 5H12.5A1.5 1.5 0 0 1 14 6.5v5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7Z" stroke="currentColor" stroke-width="1.2" fill="none"/>
              </svg>
              {{ browseLabel }}
            </button>
          </div>
        </div>
      </div>

      <p v-if="dropFeedback" class="fd-infobar fd-infobar--warning" role="alert">
        <strong>{{ dropFeedback }}</strong>
      </p>

      <div v-if="props.items.length" class="queue-shell" :class="{ 'queue-shell--active': dragActive }">
        <div class="queue-shell__header">
          <strong>{{ t('upload.queueTitle') }}</strong>
          <span>{{ t('upload.queueHint') }}</span>
        </div>

        <div class="queue-list">
          <article
            v-for="item in props.items"
            :key="item.id"
            class="queue-item"
            :class="{ 'queue-item--selected': props.selectedId === item.id }"
            role="button"
            tabindex="0"
            @click="emit('select', item.id)"
            @keydown="handleQueueItemKeydown($event, item.id)"
            @contextmenu.prevent="handleQueueItemContextMenu($event, item)"
          >
            <div class="queue-item__row">
              <div class="queue-item__info">
                <div class="queue-item__title-line">
                  <span class="fd-badge fd-badge--sm" :class="`fd-badge--${statusTone(item.status)}`">
                    {{ statusLabel(item.status) }}
                  </span>
                  <strong class="queue-item__name" :title="item.fileName">{{ item.fileName }}</strong>
                </div>
                <div class="queue-item__meta">
                  <span class="fd-badge fd-badge--sm">{{ item.presetLabel }}</span>
                  <span v-if="item.overrideLabel" class="fd-badge fd-badge--sm fd-badge--accent">
                    {{ item.overrideLabel }}
                  </span>
                  <span v-for="entry in item.meta" :key="entry" class="queue-item__meta-text">{{ entry }}</span>
                </div>
                <p class="queue-item__path" :title="item.path">{{ item.path }}</p>
              </div>
              <button
                class="queue-item__delete"
                type="button"
                :title="t('queue.delete')"
                :disabled="props.queueLocked"
                @click.stop="emit('delete', item.id)"
              >
                <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
                  <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
              </button>
            </div>
            <div class="fd-progress fd-progress--sm" aria-hidden="true">
              <span
                class="fd-progress__bar"
                :style="{ width: `${Math.max(item.progressPercent, item.status === 'success' ? 100 : 2)}%` }"
              ></span>
            </div>
          </article>
        </div>

        <div
          v-if="contextMenu"
          class="queue-context-menu"
          :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
          @click.stop
        >
          <button class="queue-context-menu__item" type="button" @click="openQueueItemResult">
            {{ t('queue.openCompressedFile') }}
          </button>
          <button class="queue-context-menu__item" type="button" @click="openQueueItemResultFolder">
            {{ t('queue.openCompressedFolder') }}
          </button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.upload-panel {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-16);
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.upload-panel--active {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-12);
}

.panel-header__left {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.panel-header__right {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.panel-header__icon {
  flex-shrink: 0;
}

.panel-header h2 {
  font: var(--fd-text-section);
}

.upload-stage {
  display: grid;
  grid-template-rows: 1fr;
  gap: var(--fd-space-12);
  min-height: 0;
  flex: 1;
}

.upload-stage--filled {
  grid-template-rows: 1fr;
}

.dropzone {
  display: flex;
  align-items: stretch;
  min-height: 0;
  height: 100%;
  padding: clamp(var(--fd-space-14), 2.2vw, var(--fd-space-24));
  border: 1.5px dashed var(--fd-stroke-divider);
  border-radius: var(--fd-radius-xl);
  background:
    radial-gradient(circle at top right, var(--fd-accent-subtle), transparent 48%),
    var(--fd-layer-2);
  transition:
    border-color var(--fd-duration-fast) var(--fd-easing-standard),
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    transform var(--fd-duration-fast) var(--fd-easing-standard);
}

.dropzone--active {
  border-color: var(--fd-accent-border);
  background:
    radial-gradient(circle at top right, var(--fd-accent-subtle), transparent 40%),
    var(--fd-layer-2);
}

.dropzone__inner {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--fd-space-16);
  width: 100%;
  min-height: 0;
  text-align: center;
}

.dropzone__icon {
  color: var(--fd-text-tertiary);
}

.dropzone--active .dropzone__icon {
  color: var(--fd-accent);
}

.dropzone__text {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-4);
}

.dropzone__text strong {
  font: var(--fd-text-section);
  color: var(--fd-text-primary);
}

.dropzone__text p {
  font: var(--fd-text-body-large);
  color: var(--fd-text-secondary);
}

.dropzone__actions {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--fd-space-8);
}

.dropzone__support {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
}

.queue-shell {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-12);
  min-height: 0;
  position: relative;
  padding: var(--fd-space-16);
  border-radius: var(--fd-radius-xl);
  background: color-mix(in srgb, var(--fd-layer-2) 92%, transparent);
  border: 1px solid var(--fd-stroke-card);
  transition:
    border-color var(--fd-duration-fast) var(--fd-easing-standard),
    box-shadow var(--fd-duration-fast) var(--fd-easing-standard),
    background-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.queue-shell--active {
  border-color: var(--fd-accent-border);
  box-shadow: var(--fd-shadow-focus);
  background:
    radial-gradient(circle at top right, var(--fd-accent-subtle), transparent 45%),
    var(--fd-layer-2);
}

.queue-shell__header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--fd-space-12);
}

.queue-shell__header strong {
  font: var(--fd-text-section);
}

.queue-shell__header span {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  max-width: none;
  white-space: nowrap;
}

.queue-list {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-10);
  min-height: 0;
  overflow: auto;
  padding-right: 0;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.queue-list::-webkit-scrollbar {
  display: none;
}

.queue-item {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
  padding: 12px 14px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: color-mix(in srgb, var(--fd-layer-1) 88%, transparent);
  appearance: none;
  text-align: left;
  cursor: pointer;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    border-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.queue-item:hover {
  border-color: var(--fd-control-stroke);
  background: var(--fd-surface-hover);
}

.queue-item--selected {
  border-color: var(--fd-accent-border);
  background: var(--fd-accent-subtle);
}

.queue-item--selected:hover {
  border-color: var(--fd-accent-border);
}

.queue-item:focus-visible {
  outline: none;
  box-shadow: var(--fd-shadow-focus);
}

.queue-item__row {
  display: flex;
  align-items: center;
  gap: var(--fd-space-10);
}

.queue-item__info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-4);
}

.queue-item__title-line {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
  min-width: 0;
}

.queue-item__delete {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: var(--fd-radius-sm);
  background: transparent;
  color: var(--fd-text-tertiary);
  cursor: pointer;
  transition:
    background-color var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard);
}

.queue-item__delete:hover {
  background: var(--fd-danger-subtle);
  border-color: var(--fd-danger-border);
  color: var(--fd-danger);
}

.queue-item__delete:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.queue-item__name {
  color: var(--fd-text-primary);
  font: var(--fd-text-body-large);
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.queue-item__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 6px;
  align-items: center;
}

.queue-item__meta-text {
  font: var(--fd-text-caption);
  color: var(--fd-text-tertiary);
}

.queue-item__path {
  margin: 0;
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fd-progress--sm {
  height: 4px;
}

.queue-context-menu {
  position: fixed;
  z-index: 30;
  display: flex;
  flex-direction: column;
  min-width: 200px;
  padding: 6px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 14px;
  background: var(--fd-flyout-bg);
  box-shadow: var(--fd-shadow-lg);
  backdrop-filter: blur(24px) saturate(150%);
  -webkit-backdrop-filter: blur(24px) saturate(150%);
}

.queue-context-menu__item {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: 34px;
  padding: 0 10px;
  border: none;
  border-radius: 10px;
  background: transparent;
  color: var(--fd-text-primary);
  font: var(--fd-text-body);
  text-align: left;
  cursor: pointer;
  transition: background-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.queue-context-menu__item:hover {
  background: var(--fd-surface-hover);
}

@media (max-width: 768px) {
  .dropzone {
    padding: var(--fd-space-14);
  }

  .queue-shell__header {
    flex-direction: column;
    align-items: flex-start;
  }

  .queue-shell__header span {
    white-space: normal;
  }
}
</style>
