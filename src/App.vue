<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ActivityPanel from './components/ActivityPanel.vue'
import AppHeader from './components/AppHeader.vue'
import AppearanceSettingsPanel from './components/AppearanceSettingsPanel.vue'
import CompressionSettingsPanel from './components/CompressionSettingsPanel.vue'
import ErrorToastViewport from './components/ErrorToastViewport.vue'
import OutputDirPanel from './components/OutputDirPanel.vue'
import PdfUploadPanel from './components/PdfUploadPanel.vue'
import PresetBar from './components/PresetBar.vue'
import QuickCompressPanel from './components/QuickCompressPanel.vue'
import { createNotice } from './composables/backendMessages'
import { usePdfCompressor } from './composables/usePdfCompressor'
import { listenForOpenPdf } from './lib/tauri'
import type { NoticeItem } from './types/pdf'
import { formatBytes, formatPercent } from './utils/format'

const {
  jobs,
  selectedJobId,
  sourceFileName,
  settings,
  analysis,
  result,
  nativeAvailable,
  analysisLoading,
  compressionLoading,
  errorToasts,
  workflowState,
  canCompress,
  canCancelCompression,
  pendingQueueCount,
  pushErrorToast,
  updateSettings,
  applySettingsToAll,
  browseForPdf,
  addSourcePaths,
  selectJob,
  removeJobById,
  compressCurrentPdf,
  compressSelectedPdf,
  cancelCompressionRun,
  selectOutputDir,
  openCompressedFile,
  openCompressedFileFolder,
  dismissErrorToast,
  pauseErrorToast,
  resumeErrorToast,
  reportError,
} = usePdfCompressor()

const { t } = useI18n()

// Top-level navigation: the queue workflow is the main view; compression and
// quick-mode (right-click) settings live on their own page.
const currentView = ref<'main' | 'settings'>('main')

function toggleSettingsView() {
  currentView.value = currentView.value === 'main' ? 'settings' : 'main'
}

const busy = computed(() => analysisLoading.value || compressionLoading.value)
const recommendedPreset = computed(() => analysis.value?.recommendedPreset ?? null)
// Target-size is its own compression mode: badges and queue labels name the
// mode instead of the underlying preset when it is active.
const activeModeLabel = computed(() =>
  settings.value.targetFileSizeMb != null ? t('settings.targetMode') : null,
)
const activeQueueCount = computed(() => jobs.value.filter((job) => job.sourcePath.trim()).length)
const completedQueueCount = computed(() => jobs.value.filter((job) => job.status === 'success').length)
const canApplySettingsToAll = computed(
  () => Boolean(selectedJobId.value) && activeQueueCount.value > 1 && !compressionLoading.value,
)
const applyToAllHint = computed(() =>
  compressionLoading.value
    ? t('upload.lockedHint')
    : activeQueueCount.value <= 1
      ? t('settings.applyToAllHintSingle')
      : null,
)
const selectedJobProgressPercent = computed(
  () => jobs.value.find((job) => job.id === selectedJobId.value)?.progress.percent ?? 0,
)
const primaryActionLabel = computed(() => {
  if (pendingQueueCount.value > 1) {
    return t('activity.startCompressionAll', { count: pendingQueueCount.value })
  }
  // Everything finished and a completed file selected: offer a re-run of
  // that file instead of a dead-end "all done" state.
  if (pendingQueueCount.value === 0 && selectedJobIsCompleted.value) {
    return t('activity.recompress')
  }
  return t('activity.startCompression')
})
// The queue-scope button compresses everything pending; when several files
// are pending and the selection itself is pending, offer the single-file
// scope as a secondary action.
const selectedJobIsPending = computed(() => {
  const job = jobs.value.find((item) => item.id === selectedJobId.value)
  return Boolean(job && job.status !== 'compressing' && job.status !== 'success')
})
const selectedJobIsCompleted = computed(
  () => jobs.value.find((item) => item.id === selectedJobId.value)?.status === 'success',
)
const secondaryActionLabel = computed(() => {
  if (compressionLoading.value || !selectedJobId.value) {
    return null
  }
  // The primary button covers the whole pending queue; offer the single-file
  // scope whenever that differs from just the selected file — several files
  // pending, or a finished/failed selection alongside pending ones (re-run
  // just this file).
  if (
    pendingQueueCount.value === 0 ||
    (pendingQueueCount.value === 1 && selectedJobIsPending.value)
  ) {
    return null
  }
  return t('activity.compressSelected')
})
// Backend notes for the selected job (analysis hints + compression report).
const selectedJobNotes = computed<NoticeItem[]>(() => {
  const job = jobs.value.find((item) => item.id === selectedJobId.value)
  if (!job) {
    return []
  }
  return [...(job.analysis?.notes ?? []), ...(job.result?.notes ?? [])]
})

function formatSizeChange(original?: number, compressed?: number): string {
  const from = formatBytes(original)
  const to = formatBytes(compressed)
  if (from === '--' || to === '--') {
    return ''
  }
  return `${from} → ${to}`
}

const queueItems = computed(() =>
  jobs.value
    .filter((job) => job.sourcePath.trim())
    .map((job) => {
      const sizeMeta = job.result
        ? formatSizeChange(
            job.result.originalSizeBytes ?? job.analysis?.fileSizeBytes,
            job.result.compressedSizeBytes,
          )
        : formatBytes(job.analysis?.fileSizeBytes) !== '--'
          ? formatBytes(job.analysis?.fileSizeBytes)
          : ''

      return {
        id: job.id,
        fileName: job.fileName || t('app.emptySource'),
        path: job.sourcePath,
        status: job.status,
        presetLabel:
          job.settings.targetFileSizeMb != null
            ? t('settings.targetMode')
            : t(`app.preset.${job.settings.preset}`),
        detail:
          job.error && job.status === 'error'
            ? job.error.body
            : t(`queue.detail.${job.status}`),
        meta: [
          sizeMeta,
          job.analysis?.pageCount ? t('queue.pageCount', { count: job.analysis.pageCount }, job.analysis.pageCount) : '',
          job.result ? formatPercent(job.result.savingsPercent) : '',
        ].filter(Boolean),
        progressPercent: job.progress.percent,
        overrideLabel: job.analysis && !job.useRecommendedSettings ? t('queue.overrideTag') : '',
        outputPath: job.result?.outputPath ?? null,
      }
    }),
)

function handlePresetSaved() {
  pushErrorToast(
    createNotice(
      'preset:saved',
      'success',
      t('settings.presetSavedTitle'),
      t('settings.presetSavedBody'),
    ),
  )
}

function handleQuickProfileSaved() {
  pushErrorToast(
    createNotice(
      'quick:saved',
      'success',
      t('quick.savedTitle'),
      t('quick.savedBody'),
    ),
  )
}

// PDFs handed to an already-running instance ("Open with…") join the queue.
let stopListeningOpenPdf: (() => void) | null = null

onMounted(async () => {
  try {
    stopListeningOpenPdf = await listenForOpenPdf((paths) => {
      addSourcePaths(paths)
    })
  } catch (error) {
    console.warn('open-pdf event listener unavailable:', error)
  }
})

onBeforeUnmount(() => {
  stopListeningOpenPdf?.()
  stopListeningOpenPdf = null
})
</script>

<template>
  <div class="app-shell" :aria-busy="busy ? 'true' : 'false'">
    <AppHeader
      :native-available="nativeAvailable"
      :view="currentView"
      @toggle-settings="toggleSettingsView"
    />
    <ErrorToastViewport
      :items="errorToasts"
      @dismiss="dismissErrorToast"
      @pause="pauseErrorToast"
      @resume="resumeErrorToast"
    />

    <main v-if="currentView === 'main'" class="shell-content">
      <div class="shell-frame">
        <section class="shell-stage">
          <div class="shell-stage__panel">
            <PdfUploadPanel
              :items="queueItems"
              :selected-id="selectedJobId"
              :queue-locked="compressionLoading"
              :native-available="nativeAvailable"
              @add-paths="addSourcePaths"
              @browse="browseForPdf"
              @select="selectJob"
              @delete="removeJobById"
              @open-result="openCompressedFile"
              @open-result-folder="openCompressedFileFolder"
            />
          </div>
        </section>

        <aside class="shell-rail fd-card">
          <OutputDirPanel
            :output-dir="settings.outputDir"
            :native-available="nativeAvailable"
            :queue-locked="compressionLoading"
            @select="selectOutputDir"
          />
          <PresetBar
            :settings="settings"
            :disabled="compressionLoading"
            :can-apply-to-all="canApplySettingsToAll"
            :apply-to-all-hint="applyToAllHint"
            :recommended-preset="recommendedPreset"
            @update:settings="updateSettings"
            @apply-settings-to-all="applySettingsToAll"
          />
          <ActivityPanel
            :workflow-state="workflowState"
            :selected-file-name="sourceFileName"
            :selected-preset="settings.preset"
            :mode-label="activeModeLabel"
            :analysis="analysis"
            :result="result"
            :primary-action-label="primaryActionLabel"
            :primary-action-disabled="!canCompress"
            :secondary-action-label="secondaryActionLabel"
            :can-cancel="canCancelCompression"
            :queue-count="activeQueueCount"
            :pending-count="pendingQueueCount"
            :completed-count="completedQueueCount"
            :progress-percent="selectedJobProgressPercent"
            :notes="selectedJobNotes"
            :queue-locked="compressionLoading"
            @primary-action="compressCurrentPdf"
            @secondary-action="compressSelectedPdf"
            @cancel-action="cancelCompressionRun"
          />
        </aside>
      </div>
    </main>

    <main v-else class="shell-content shell-content--settings">
      <div class="settings-page">
        <div class="settings-page__heading">
          <h1>{{ t('settingsView.title') }}</h1>
        </div>

        <div class="settings-grid">
          <div class="settings-card fd-card settings-card--appearance">
            <AppearanceSettingsPanel />
          </div>

          <div class="settings-col">
            <div class="settings-card fd-card">
              <CompressionSettingsPanel
                :settings="settings"
                :disabled="compressionLoading"
                :analysis="analysis"
                @update:settings="updateSettings"
                @preset-config-error="reportError"
                @preset-config-saved="handlePresetSaved"
              />
            </div>
          </div>

          <div class="settings-col">
            <div class="settings-card fd-card">
              <QuickCompressPanel
                :native-available="nativeAvailable"
                @saved="handleQuickProfileSaved"
                @error="reportError"
              />
            </div>
          </div>
        </div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

.shell-content {
  flex: 1;
  display: flex;
  min-height: 0;
  padding: var(--fd-space-16) var(--fd-space-20) var(--fd-space-20);
  overflow: hidden;
}

/* Main view: the queue is the hero; the activity rail stays docked right. */
.shell-frame {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 420px;
  gap: var(--fd-space-16);
  width: 100%;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  align-items: stretch;
}

.shell-stage,
.shell-rail {
  min-width: 0;
  min-height: 0;
  height: 100%;
}

.shell-stage {
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow: hidden;
}

.shell-stage__panel {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.shell-rail {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-10);
  padding: var(--fd-space-12);
  overflow: hidden;
}

.shell-rail :deep(.activity-panel) {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.shell-stage__panel :deep(.upload-panel) {
  flex: 1;
  height: 100%;
  min-height: 0;
}

/* Settings view: appearance spans a full row on top; below it the
   compression preset and the right-click quick preset sit as symmetric
   equal-width cards. The grid fills the viewport height so a maximized
   window stretches the cards instead of leaving bottom whitespace. */
.shell-content--settings {
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}

.settings-page {
  display: flex;
  flex-direction: column;
  flex: 1;
  gap: var(--fd-space-16);
  /* Full width — the view scales with the window, maximized included. */
  width: 100%;
  margin: 0 auto;
  padding-bottom: var(--fd-space-20);
}

.settings-page__heading h1 {
  font: var(--fd-text-subtitle);
}

.settings-grid {
  display: grid;
  flex: 1;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  /* Appearance keeps its natural toolbar height; the two preset cards
     absorb all remaining height between them. */
  grid-template-rows: auto minmax(0, 1fr);
  gap: var(--fd-space-16);
  align-items: stretch;
}

.settings-card--appearance {
  grid-column: 1 / -1;
}

.settings-col {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-16);
  min-width: 0;
}

/* Whichever of the two preset cards ends up shorter stretches so the row
   reads as one symmetric block: the compression panel's advanced section
   and the quick panel's footer both absorb the extra height. */
.settings-col > .settings-card:last-child {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.settings-col > .settings-card:last-child :deep(.settings-dock),
.settings-col > .settings-card:last-child :deep(.quick-panel) {
  flex: 1;
  min-height: 0;
}

.settings-card {
  padding: var(--fd-space-20);
}

.settings-card :deep(.settings-dock) {
  height: auto;
}

@media (max-width: 980px) {
  .settings-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 1080px) {
  .shell-frame {
    grid-template-columns: minmax(0, 1fr) 360px;
    gap: var(--fd-space-12);
  }
}

@media (max-width: 768px) {
  .shell-content {
    padding: var(--fd-space-12);
  }

  .shell-frame {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) auto;
    gap: var(--fd-space-12);
    overflow-y: auto;
  }

  .shell-stage__panel {
    min-height: 320px;
  }

  .shell-rail {
    height: auto;
  }
}
</style>
