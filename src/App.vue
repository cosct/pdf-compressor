<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import ActivityPanel from './components/ActivityPanel.vue'
import AppHeader from './components/AppHeader.vue'
import CompressionSettingsPanel from './components/CompressionSettingsPanel.vue'
import ErrorToastViewport from './components/ErrorToastViewport.vue'
import PdfUploadPanel from './components/PdfUploadPanel.vue'
import { createNotice } from './composables/backendMessages'
import { usePdfCompressor } from './composables/usePdfCompressor'
import { appLocales, setAppLocale, type AppLocale } from './i18n'
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

const { t, locale } = useI18n()

const busy = computed(() => analysisLoading.value || compressionLoading.value)
const recommendedPreset = computed(() => analysis.value?.recommendedPreset ?? null)
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
const primaryActionLabel = computed(() =>
  pendingQueueCount.value > 1
    ? t('activity.startCompressionAll', { count: pendingQueueCount.value })
    : t('activity.startCompression'),
)
// The queue-scope button compresses everything pending; when several files
// are pending and the selection itself is pending, offer the single-file
// scope as a secondary action.
const selectedJobIsPending = computed(() => {
  const job = jobs.value.find((item) => item.id === selectedJobId.value)
  return Boolean(job && job.status !== 'compressing' && job.status !== 'success')
})
const secondaryActionLabel = computed(() =>
  pendingQueueCount.value > 1 && selectedJobIsPending.value && !compressionLoading.value
    ? t('activity.compressSelected')
    : null,
)
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
        presetLabel: t(`app.preset.${job.settings.preset}`),
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

function updateLocale(nextLocale: AppLocale) {
  setAppLocale(nextLocale)
}

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
      :locale="locale as AppLocale"
      :locales="appLocales"
      @update:locale="updateLocale"
    />
    <ErrorToastViewport
      :items="errorToasts"
      @dismiss="dismissErrorToast"
      @pause="pauseErrorToast"
      @resume="resumeErrorToast"
    />

    <main class="shell-content">
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

        <section class="shell-dock fd-card fd-card--acrylic">
          <div class="shell-dock__panel shell-dock__panel--settings">
            <CompressionSettingsPanel
              :settings="settings"
              :disabled="compressionLoading"
              :can-apply-to-all="canApplySettingsToAll"
              :apply-to-all-hint="applyToAllHint"
              :recommended-preset="recommendedPreset"
              :analysis="analysis"
              @update:settings="updateSettings"
              @apply-settings-to-all="applySettingsToAll"
              @preset-config-error="reportError"
              @preset-config-saved="handlePresetSaved"
            />
          </div>

          <div class="shell-dock__panel shell-dock__panel--activity">
            <ActivityPanel
              :workflow-state="workflowState"
              :selected-file-name="sourceFileName"
              :selected-preset="settings.preset"
              :recommended-preset="recommendedPreset"
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
              :output-dir="settings.outputDir"
              :native-available="nativeAvailable"
              :queue-locked="compressionLoading"
              @primary-action="compressCurrentPdf"
              @secondary-action="compressSelectedPdf"
              @cancel-action="cancelCompressionRun"
              @select-output-dir="selectOutputDir"
            />
          </div>
        </section>
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
  padding: var(--fd-space-14) var(--fd-space-20) var(--fd-space-20);
  overflow: hidden;
}

.shell-frame {
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  gap: var(--fd-space-16);
  width: 100%;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  align-items: stretch;
}

.shell-stage,
.shell-dock {
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

.shell-dock {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-12);
  padding: var(--fd-space-14);
  /* Fixed per user preference; the advanced settings section scrolls
     internally if its content exceeds this height. */
  height: 380px;
  min-height: 0;
  overflow: hidden;
}

.shell-dock__panel {
  display: flex;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.shell-dock__panel :deep(.settings-dock),
.shell-dock__panel :deep(.activity-panel) {
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.shell-stage__panel :deep(.upload-panel) {
  flex: 1;
  height: 100%;
  min-height: 0;
}

@media (max-width: 1180px) {
  .shell-dock {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--fd-space-14);
    padding: var(--fd-space-16);
  }
}

@media (max-width: 768px) {
  .shell-content {
    padding: var(--fd-space-12);
  }

  .shell-frame {
    gap: var(--fd-space-12);
  }

  .shell-stage__panel {
    min-height: 0;
  }

  .shell-dock {
    grid-template-columns: 1fr;
    gap: var(--fd-space-12);
    height: auto;
    padding: var(--fd-space-12);
  }
}
</style>
