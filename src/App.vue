<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import ActivityPanel from './components/ActivityPanel.vue'
import AppHeader from './components/AppHeader.vue'
import CompressionSettingsPanel from './components/CompressionSettingsPanel.vue'
import ErrorToastViewport from './components/ErrorToastViewport.vue'
import PdfUploadPanel from './components/PdfUploadPanel.vue'
import { usePdfCompressor } from './composables/usePdfCompressor'
import { appLocales, setAppLocale, type AppLocale } from './i18n'
import { listenForOpenPdf } from './lib/tauri'
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
  updateSettings,
  applySettingsToAll,
  browseForPdf,
  addSourcePaths,
  selectJob,
  removeJobById,
  compressCurrentPdf,
  cancelCompressionRun,
  selectOutputDir,
  openCompressedFile,
  openCompressedFileFolder,
  dismissErrorToast,
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
const selectedJobProgressPercent = computed(
  () => jobs.value.find((job) => job.id === selectedJobId.value)?.progress.percent ?? 0,
)

function formatSizeChange(original?: number, compressed?: number): string {
  const from = formatBytes(original)
  const to = formatBytes(compressed)
  if (from === '--' || to === '--') {
    return ''
  }
  return `${from} -> ${to}`
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
        status: job.status === 'idle' ? 'selected' : job.status,
        presetLabel: t(`app.preset.${job.settings.preset}`),
        detail:
          job.error && job.status === 'error'
            ? job.error.body
            : t(`queue.detail.${job.status === 'idle' ? 'selected' : job.status}`),
        meta: [
          sizeMeta,
          job.analysis?.pageCount ? `${job.analysis.pageCount} ${t('queue.pages')}` : '',
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
    <ErrorToastViewport :items="errorToasts" @dismiss="dismissErrorToast" />

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
              :recommended-preset="recommendedPreset"
              :analysis="analysis"
              @update:settings="updateSettings"
              @apply-settings-to-all="applySettingsToAll"
              @preset-config-error="reportError"
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
              :primary-action-label="t('activity.startCompression')"
              :primary-action-disabled="!canCompress"
              :can-cancel="canCancelCompression"
              :queue-count="activeQueueCount"
              :completed-count="completedQueueCount"
              :progress-percent="selectedJobProgressPercent"
              :output-dir="settings.outputDir"
              :native-available="nativeAvailable"
              :queue-locked="compressionLoading"
              @primary-action="compressCurrentPdf"
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
  height: clamp(304px, 36vh, 352px);
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
