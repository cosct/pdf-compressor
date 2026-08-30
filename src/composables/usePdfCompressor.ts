/**
 * Core workflow composable — single source of truth for the PDF compression pipeline.
 * 核心工作流 composable — PDF 压缩流水线的唯一状态来源。
 *
 * Manages the job queue, per-job settings, loading/error states, and concurrent
 * compression scheduling. Message sanitization/localization lives in
 * `backendMessages.ts`; toast state lives in `useErrorToasts.ts`.
 * 管理任务队列、逐任务设置、加载/错误状态和并发压缩调度。
 * 消息净化/本地化位于 backendMessages.ts；提示状态位于 useErrorToasts.ts。
 */
import { computed, ref, watch } from 'vue'

import { getPresetDefaults } from '../config/presets'
import { translate } from '../i18n'
import {
  analyzePdf,
  cancelCompression,
  compressPdf,
  compressScannedPdf,
  existingPaths,
  hasNativeCommands,
  openDirectoryDialog,
  openPath,
  openPdfDialog,
  revealPathInFolder,
} from '../lib/tauri'
import type {
  AnalysisSummary,
  CompressionPreset,
  CompressionSettings,
  PdfQueueJob,
  ProgressUpdate,
  QueueItemStatus,
  WorkflowState,
} from '../types/pdf'
import {
  clampImageQuality,
  clampMaxImageSizePercent,
  normalizeReferenceMaxImageEdgePx,
} from '../utils/compressionSettings'
import { fileNameFromPath, isPdfPath } from '../utils/format'
import {
  clampPercent,
  createNotice,
  isCancellationError,
  mapAnalysisSummary,
  mapCompressionResult,
  normalizeError,
} from './backendMessages'
import { useErrorToasts } from './useErrorToasts'

function createSettingsForPreset(
  preset: CompressionPreset,
  referenceMaxImageEdgePx?: number | null,
  overrides?: Partial<Omit<CompressionSettings, 'preset'>>,
): CompressionSettings {
  const presetDefaults = getPresetDefaults(preset)
  const normalizedReferenceMaxImageEdgePx = normalizeReferenceMaxImageEdgePx(
    overrides?.referenceMaxImageEdgePx ?? referenceMaxImageEdgePx,
  )

  return normalizeSettings({
    preset,
    imageQuality: overrides?.imageQuality ?? presetDefaults.imageQuality,
    maxImageSizePercent: overrides?.maxImageSizePercent ?? presetDefaults.maxImageSizePercent,
    referenceMaxImageEdgePx: normalizedReferenceMaxImageEdgePx,
    optimizeImages: overrides?.optimizeImages ?? presetDefaults.optimizeImages,
    compressStreams: overrides?.compressStreams ?? presetDefaults.compressStreams,
    stripMetadata: overrides?.stripMetadata ?? presetDefaults.stripMetadata,
    grayscale: overrides?.grayscale ?? presetDefaults.grayscale,
    bilevelCodec: overrides?.bilevelCodec ?? presetDefaults.bilevelCodec,
    subsetFonts: overrides?.subsetFonts ?? presetDefaults.subsetFonts,
    outputDir: overrides?.outputDir ?? null,
    targetFileSizeMb: overrides?.targetFileSizeMb ?? null,
  })
}

function normalizeTargetFileSizeMb(value: number | null | undefined): number | null {
  if (value === null || value === undefined || !Number.isFinite(value) || value <= 0) {
    return null
  }
  return Math.min(2048, Math.max(0.1, value))
}

export function normalizeSettings(settings: CompressionSettings): CompressionSettings {
  const preset = settings.preset ?? 'balanced'

  return {
    ...settings,
    preset,
    imageQuality: clampImageQuality(
      Number.isFinite(settings.imageQuality)
        ? settings.imageQuality
        : getPresetDefaults(preset).imageQuality,
    ),
    maxImageSizePercent: clampMaxImageSizePercent(
      Number.isFinite(settings.maxImageSizePercent)
        ? settings.maxImageSizePercent
        : getPresetDefaults(preset).maxImageSizePercent,
    ),
    referenceMaxImageEdgePx: normalizeReferenceMaxImageEdgePx(settings.referenceMaxImageEdgePx),
    // Restored legacy queues may predate the grayscale field, so coerce
    // `undefined` back to the default instead of trusting the stored shape.
    grayscale: settings.grayscale ?? false,
    bilevelCodec: settings.bilevelCodec === 'ccitt-g4' ? 'ccitt-g4' : 'jpeg',
    subsetFonts: settings.subsetFonts ?? false,
    outputDir: settings.outputDir?.trim() ? settings.outputDir.trim() : null,
    targetFileSizeMb: normalizeTargetFileSizeMb(settings.targetFileSizeMb),
  }
}

export function getCompressionConcurrency(jobCount: number): number {
  const cpu = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 4 : 4
  if (jobCount <= 1) {
    return 1
  }

  return Math.min(2, Math.max(1, Math.floor(cpu / 2)))
}

function mapProgressPhaseToWorkflow(phase: ProgressUpdate['phase']): QueueItemStatus {
  switch (phase) {
    case 'analyzing':
      return 'analyzing'
    case 'compressing':
    case 'writing':
      return 'compressing'
    case 'done':
      return 'success'
    case 'error':
      return 'error'
    default:
      return 'selected'
  }
}

function createJob(path: string): PdfQueueJob {
  const normalizedPath = path.trim()
  const defaults = createSettingsForPreset('balanced')

  return {
    id: `${normalizedPath}::${Date.now()}::${Math.random().toString(36).slice(2, 8)}`,
    sourcePath: normalizedPath,
    fileName: fileNameFromPath(normalizedPath),
    status: 'selected',
    progress: {
      phase: 'queued',
      percent: 0,
    },
    analysis: null,
    result: null,
    settings: { ...defaults },
    recommendedSettings: { ...defaults },
    useRecommendedSettings: true,
    settingsRestored: false,
    error: null,
  }
}

function createRecommendedSettings(analysis: AnalysisSummary | null): CompressionSettings {
  const preset = analysis?.recommendedPreset ?? 'balanced'
  return createSettingsForPreset(preset, analysis?.maxImageEdgePx ?? undefined)
}

function comparableCompressionSettings(settings: CompressionSettings) {
  const normalized = normalizeSettings(settings)

  return {
    imageQuality: normalized.imageQuality,
    maxImageSizePercent: normalized.maxImageSizePercent,
    optimizeImages: normalized.optimizeImages,
    compressStreams: normalized.compressStreams,
    stripMetadata: normalized.stripMetadata,
    grayscale: normalized.grayscale,
    bilevelCodec: normalized.bilevelCodec,
    subsetFonts: normalized.subsetFonts,
    targetFileSizeMb: normalized.targetFileSizeMb,
  }
}

// ---------------------------------------------------------------------------
// Queue persistence — remember source paths + per-job settings across restarts
// 队列持久化 — 重启后恢复文件列表与逐任务设置
// ---------------------------------------------------------------------------

const QUEUE_STORAGE_KEY = 'pdf-compressor-queue'

type PersistedQueueEntry = {
  sourcePath: string
  settings: CompressionSettings
}

function persistQueue(entries: PersistedQueueEntry[]) {
  try {
    window.localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(entries))
  } catch {
    // Storage unavailable — persistence is best-effort only.
  }
}

function readPersistedQueue(): PersistedQueueEntry[] {
  try {
    const raw = window.localStorage.getItem(QUEUE_STORAGE_KEY)
    if (!raw) {
      return []
    }
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) {
      return []
    }
    return parsed.filter(
      (entry): entry is PersistedQueueEntry =>
        entry !== null &&
        typeof entry === 'object' &&
        typeof entry.sourcePath === 'string' &&
        entry.sourcePath.trim().length > 0 &&
        entry.settings !== null &&
        typeof entry.settings === 'object',
    )
  } catch {
    return []
  }
}

export function usePdfCompressor() {
  const { errorToasts, pushErrorToast, dismissErrorToast, pauseErrorToast, resumeErrorToast, reportError } = useErrorToasts()

  const jobs = ref<PdfQueueJob[]>([])
  const selectedJobId = ref<string | null>(null)
  const draftSettings = ref(createSettingsForPreset('balanced'))
  const draftUsesRecommended = ref(false)
  const nativeAvailable = hasNativeCommands()
  const analysisQueue = ref<string[]>([])
  const analysisRunning = ref(false)
  const compressionRunning = ref(false)
  const cancellationRequested = ref(false)
  let compressionRunSerial = 0
  let currentCompressionRunId: number | null = null
  let analysisQueuePromise: Promise<void> | null = null
  const activeCompressionTaskIds = new Map<string, string>()
  const cancelledCompressionRuns = new Set<number>()

  const selectedJob = computed(() =>
    jobs.value.find((job) => job.id === selectedJobId.value) ?? null,
  )
  const settings = computed(() => selectedJob.value?.settings ?? draftSettings.value)
  const sourceFileName = computed(() => selectedJob.value?.fileName ?? '')
  const analysis = computed(() => selectedJob.value?.analysis ?? null)
  const result = computed(() => selectedJob.value?.result ?? null)
  const analysisLoading = computed(() => jobs.value.some((job) => job.status === 'analyzing'))
  const compressionLoading = computed(() => jobs.value.some((job) => job.status === 'compressing'))
  const workflowState = computed<WorkflowState>(() => selectedJob.value?.status ?? 'idle')
  const jobsPendingCompression = computed(() =>
    jobs.value.filter(
      (job) =>
        isPdfPath(job.sourcePath) &&
        job.status !== 'compressing' &&
        job.status !== 'success',
    ),
  )
  const pendingQueueCount = computed(() => jobsPendingCompression.value.length)
  const selectedCompressionTargetIds = computed(() => {
    if (
      !selectedJob.value ||
      !isPdfPath(selectedJob.value.sourcePath) ||
      selectedJob.value.status === 'compressing'
    ) {
      return []
    }

    return [selectedJob.value.id]
  })
  const allCompressionTargetIds = computed(() =>
    jobsPendingCompression.value.map((job) => job.id),
  )
  const canCompress = computed(
    () =>
      !compressionRunning.value &&
      (allCompressionTargetIds.value.length > 0 || selectedCompressionTargetIds.value.length > 0),
  )
  const canCancelCompression = computed(() => compressionRunning.value)

  function findJob(jobId: string): PdfQueueJob | undefined {
    return jobs.value.find((job) => job.id === jobId)
  }

  function isActiveCompressionTask(jobId: string, taskId: string): boolean {
    return activeCompressionTaskIds.get(jobId) === taskId
  }

  function selectJob(jobId: string) {
    if (findJob(jobId)) {
      selectedJobId.value = jobId
    }
  }

  function setJobStatus(job: PdfQueueJob, status: QueueItemStatus) {
    job.status = status
  }

  function applyProgress(job: PdfQueueJob, update: ProgressUpdate) {
    job.progress = {
      phase: update.phase,
      percent: clampPercent(update.percent),
    }

    if (update.phase !== 'done') {
      setJobStatus(job, mapProgressPhaseToWorkflow(update.phase))
    }
  }

  function settingsMatch(left: CompressionSettings, right: CompressionSettings): boolean {
    return JSON.stringify(comparableCompressionSettings(left)) === JSON.stringify(comparableCompressionSettings(right))
  }

  function applyRecommendedSettings(job: PdfQueueJob) {
    const outputDir = job.settings.outputDir ?? draftSettings.value.outputDir
    // A target size is a user-given byte budget, orthogonal to the
    // analysis-recommended quality knobs — applying a recommendation must
    // not silently drop it (same rule as the output directory).
    const targetFileSizeMb = job.settings.targetFileSizeMb ?? draftSettings.value.targetFileSizeMb
    job.recommendedSettings = {
      ...createRecommendedSettings(job.analysis),
      outputDir,
    }
    // A job restored from the persisted queue keeps its previous settings —
    // they are the user's earlier choices, not a fresh draft. The flag is
    // one-shot: any later re-analysis behaves like a newly added file.
    if (job.settingsRestored) {
      job.settingsRestored = false
      job.useRecommendedSettings = settingsMatch(job.settings, job.recommendedSettings)
      return
    }
    job.settings = { ...job.recommendedSettings, targetFileSizeMb }
    job.useRecommendedSettings = true
  }

  function getSelectedCompressionTargetIds(): string[] {
    return [...selectedCompressionTargetIds.value]
  }

  function getAllCompressionTargetIds(): string[] {
    return [...allCompressionTargetIds.value]
  }

  function getPrimaryCompressionTargetIds(): string[] {
    if (allCompressionTargetIds.value.length > 0) {
      return getAllCompressionTargetIds()
    }

    return getSelectedCompressionTargetIds()
  }

  function addSourcePaths(paths: string[]) {
    if (compressionRunning.value) {
      pushErrorToast(
        createNotice(
          'queue:locked',
          'warning',
          translate('upload.lockedTag'),
          translate('composable.errors.queueLocked'),
        ),
      )
      return
    }

    const normalized = paths.map((path) => path.trim()).filter(Boolean)
    if (!normalized.length) {
      return
    }

    const existingPaths = new Set(jobs.value.map((job) => job.sourcePath.toLowerCase()))
    let nextSelectedId: string | null = selectedJobId.value

    for (const path of normalized) {
      const lowered = path.toLowerCase()
      if (existingPaths.has(lowered)) {
        continue
      }

      const job = createJob(path)
      job.settings = { ...draftSettings.value }
      job.recommendedSettings = { ...draftSettings.value }
      job.useRecommendedSettings = draftUsesRecommended.value
      jobs.value.push(job)
      existingPaths.add(lowered)
      nextSelectedId ??= job.id

      if (isPdfPath(job.sourcePath)) {
        queueJobAnalysis(job.id)
      }
    }

    if (nextSelectedId) {
      selectedJobId.value = nextSelectedId
    }
  }

  function removeJobById(jobId: string) {
    if (compressionRunning.value) {
      return
    }

    const index = jobs.value.findIndex((job) => job.id === jobId)
    if (index === -1) {
      return
    }

    jobs.value.splice(index, 1)
    if (selectedJobId.value === jobId) {
      const nextJob = jobs.value[index] ?? jobs.value[index - 1] ?? null
      selectedJobId.value = nextJob?.id ?? null
    }
  }

  function updateSettings(next: CompressionSettings) {
    const normalized = normalizeSettings(next)

    if (!selectedJob.value) {
      draftSettings.value = normalized
      draftUsesRecommended.value = false
      return
    }

    selectedJob.value.settings = normalized
    selectedJob.value.useRecommendedSettings = settingsMatch(
      normalized,
      selectedJob.value.recommendedSettings,
    )
  }

  function applySettingsToAll() {
    if (!selectedJob.value) {
      return
    }

    const currentSettings = { ...normalizeSettings(selectedJob.value.settings) }
    for (const job of jobs.value) {
      if (job.sourcePath.trim() && job.id !== selectedJobId.value) {
        job.settings = { ...currentSettings }
        job.useRecommendedSettings = settingsMatch(job.settings, job.recommendedSettings)
      }
    }
  }

  async function browseForPdf() {
    if (!nativeAvailable) {
      pushErrorToast(
        createNotice(
          'browse:desktop',
          'danger',
          translate('app.alertTitle'),
          translate('composable.errors.browseRequiresDesktop'),
        ),
      )
      return
    }

    try {
      addSourcePaths(await openPdfDialog(true))
    } catch (error) {
      reportError(error)
    }
  }

  function queueJobAnalysis(jobId: string) {
    if (!analysisQueue.value.includes(jobId)) {
      analysisQueue.value.push(jobId)
    }

    void drainAnalysisQueue()
  }

  async function drainAnalysisQueue() {
    if (analysisRunning.value) {
      return analysisQueuePromise
    }

    analysisRunning.value = true
    analysisQueuePromise = (async () => {
      try {
        while (analysisQueue.value.length) {
          const nextJobId = analysisQueue.value.shift()
          if (!nextJobId) {
            continue
          }

          await runAnalysis(nextJobId)
        }
      } finally {
        analysisRunning.value = false
        analysisQueuePromise = null
      }
    })()

    return analysisQueuePromise
  }

  async function runAnalysis(jobId: string) {
    const job = findJob(jobId)
    if (!job || !isPdfPath(job.sourcePath) || job.status === 'compressing') {
      return
    }

    const requestedPath = job.sourcePath
    job.error = null
    job.result = null
    applyProgress(job, { phase: 'analyzing', percent: 0 })

    try {
      const response = await analyzePdf(requestedPath, (update) => {
        if (job.sourcePath === requestedPath) {
          applyProgress(job, update)
        }
      })

      if (job.sourcePath !== requestedPath) {
        return
      }

      job.analysis = mapAnalysisSummary(response, requestedPath)
      applyRecommendedSettings(job)
      // Analysis done ≠ compressed: reset the progress so the bar reads 0%
      // until an actual compression run drives it to 100%.
      job.progress = { phase: 'queued', percent: 0 }
      setJobStatus(job, 'ready')
    } catch (error) {
      if (job.sourcePath !== requestedPath) {
        return
      }

      job.error = normalizeError(error)
      pushErrorToast(job.error)
      job.progress = { phase: 'error', percent: 0 }
      setJobStatus(job, 'error')
    }
  }

  async function compressJob(jobId: string, runId: number) {
    const job = findJob(jobId)
    if (!job || !isPdfPath(job.sourcePath)) {
      return
    }

    if (cancellationRequested.value) {
      return
    }

    if (!job.analysis) {
      await runAnalysis(job.id)
      if (!job.analysis) {
        return
      }
    }

    const requestedPath = job.sourcePath
    const taskId = `${job.id}::run-${runId}`
    job.error = null
    job.result = null
    activeCompressionTaskIds.set(job.id, taskId)
    applyProgress(job, { phase: 'compressing', percent: 0 })

    try {
      // Scan-heavy documents go through the dedicated scanned pipeline, which
      // forces image + stream optimization on the backend side.
      const runCompression =
        job.analysis?.documentKind === 'scan-heavy' ? compressScannedPdf : compressPdf
      if (runCompression === compressScannedPdf) {
        pushErrorToast(
          createNotice(
            'scan:pipeline',
            'neutral',
            translate('composable.notices.scanPipelineTitle'),
            translate('composable.notices.scanPipelineBody'),
          ),
        )
      }
      const response = await runCompression(requestedPath, job.settings, taskId, (update) => {
        if (job.sourcePath === requestedPath && isActiveCompressionTask(job.id, taskId)) {
          applyProgress(job, update)
        }
      })

      if (job.sourcePath !== requestedPath || !isActiveCompressionTask(job.id, taskId)) {
        return
      }

      if (cancelledCompressionRuns.has(runId) || cancellationRequested.value) {
        job.error = null
        job.result = null
        job.progress = { phase: 'queued', percent: 0 }
        setJobStatus(job, job.analysis ? 'ready' : 'selected')
        return
      }

      job.result = mapCompressionResult(response, requestedPath)
      job.progress = { phase: 'done', percent: 100 }
      setJobStatus(job, 'success')

      // Warning-level backend notes (e.g. target size missed) must not stay
      // buried in the notes list — surface them immediately as toasts.
      for (const note of job.result.notes) {
        if (note.tone === 'warning' || note.tone === 'danger') {
          pushErrorToast(note)
        }
      }
    } catch (error) {
      if (job.sourcePath !== requestedPath || !isActiveCompressionTask(job.id, taskId)) {
        return
      }

      if (isCancellationError(error)) {
        job.error = null
        job.progress = { phase: 'queued', percent: 0 }
        setJobStatus(job, job.analysis ? 'ready' : 'selected')
        return
      }

      job.error = normalizeError(error)
      pushErrorToast(job.error)
      // A failed run is not "complete" — keep the progress bar at 0% so only
      // a finished compression ever reads 100%.
      job.progress = { phase: 'error', percent: 0 }
      setJobStatus(job, 'error')
    } finally {
      if (isActiveCompressionTask(job.id, taskId)) {
        activeCompressionTaskIds.delete(job.id)
      }
    }
  }

  async function runCompressionTargets(targetIds: string[]) {
    if (!targetIds.length || compressionRunning.value) {
      return
    }

    if (analysisQueuePromise) {
      await analysisQueuePromise
    }

    compressionRunning.value = true
    cancellationRequested.value = false
    const runId = ++compressionRunSerial
    currentCompressionRunId = runId
    cancelledCompressionRuns.delete(runId)
    const concurrency = getCompressionConcurrency(targetIds.length)
    let cursor = 0

    try {
      await Promise.all(
        Array.from({ length: Math.min(concurrency, targetIds.length) }, async () => {
          while (cursor < targetIds.length) {
            if (cancellationRequested.value) {
              return
            }

            const nextId = targetIds[cursor]
            cursor += 1
            selectJob(nextId)
            await compressJob(nextId, runId)
          }
        }),
      )
    } finally {
      if (currentCompressionRunId === runId) {
        compressionRunning.value = false
        cancellationRequested.value = false
        currentCompressionRunId = null
      }

      cancelledCompressionRuns.delete(runId)
    }
  }

  async function compressCurrentPdf() {
    if (!canCompress.value) {
      return
    }

    await runCompressionTargets(getPrimaryCompressionTargetIds())
  }

  /** Compress only the selected job — the queue-scope sibling of the above. */
  async function compressSelectedPdf() {
    if (!canCompress.value || !selectedJob.value) {
      return
    }

    await runCompressionTargets(getSelectedCompressionTargetIds())
  }

  async function cancelCompressionRun() {
    if (!compressionRunning.value) {
      return
    }

    cancellationRequested.value = true
    if (currentCompressionRunId !== null) {
      cancelledCompressionRuns.add(currentCompressionRunId)
    }

    const activeIds = [...activeCompressionTaskIds.values()]

    for (const job of jobs.value) {
      if (job.status === 'compressing') {
        job.error = null
        job.result = null
        job.progress = { phase: 'queued', percent: 0 }
        setJobStatus(job, job.analysis ? 'ready' : 'selected')
      }
    }

    if (!activeIds.length) {
      compressionRunning.value = false
      currentCompressionRunId = null
      return
    }

    compressionRunning.value = false
    currentCompressionRunId = null
    activeCompressionTaskIds.clear()
    await Promise.allSettled(activeIds.map((taskId) => cancelCompression(taskId)))
  }

  async function selectOutputDir() {
    if (!nativeAvailable || compressionRunning.value) {
      return
    }

    try {
      const selected = await openDirectoryDialog()
      if (!selected) {
        return
      }

      if (!selectedJob.value) {
        draftSettings.value = normalizeSettings({
          ...draftSettings.value,
          outputDir: selected,
        })
        return
      }

      selectedJob.value.settings = normalizeSettings({
        ...selectedJob.value.settings,
        outputDir: selected,
      })
      selectedJob.value.recommendedSettings = {
        ...selectedJob.value.recommendedSettings,
        outputDir: selected,
      }
      selectedJob.value.useRecommendedSettings = settingsMatch(
        selectedJob.value.settings,
        selectedJob.value.recommendedSettings,
      )
    } catch (error) {
      // The dialog itself signals cancellation by resolving with null;
      // a rejection here is a genuine backend failure worth surfacing.
      reportError(error)
    }
  }

  async function openCompressedFile(jobId: string) {
    const job = findJob(jobId)
    const outputPath = job?.result?.outputPath
    if (!job || !outputPath || !nativeAvailable) {
      return
    }

    try {
      await openPath(outputPath)
    } catch (error) {
      reportError(error)
    }
  }

  async function openCompressedFileFolder(jobId: string) {
    const job = findJob(jobId)
    const outputPath = job?.result?.outputPath
    if (!job || !outputPath || !nativeAvailable) {
      return
    }

    try {
      await revealPathInFolder(outputPath)
    } catch (error) {
      reportError(error)
    }
  }

  // --- Queue persistence: throttle writes (progress ticks mutate jobs often) ---
  let persistTimer: ReturnType<typeof setTimeout> | null = null
  watch(
    jobs,
    (list) => {
      if (persistTimer) {
        clearTimeout(persistTimer)
      }
      persistTimer = setTimeout(() => {
        persistTimer = null
        persistQueue(
          list
            .filter((job) => job.sourcePath.trim() && isPdfPath(job.sourcePath))
            .map((job) => ({
              sourcePath: job.sourcePath,
              settings: { ...normalizeSettings(job.settings) },
            })),
        )
      }, 800)
    },
    { deep: true },
  )

  // --- Restore the previous session's queue (paths + settings only) ---
  if (!jobs.value.length) {
    void restorePersistedQueue()
  }

  async function restorePersistedQueue() {
    const persisted = readPersistedQueue()
    if (!persisted.length) {
      return
    }

    // Pre-check existence so files moved or deleted since the last session
    // are skipped up front instead of failing analysis one by one.
    let entries = persisted
    if (nativeAvailable) {
      try {
        const existing = new Set(await existingPaths(persisted.map((entry) => entry.sourcePath)))
        const stillPresent = persisted.filter((entry) => existing.has(entry.sourcePath))
        const missingCount = persisted.length - stillPresent.length

        if (missingCount > 0 && stillPresent.length > 0) {
          pushErrorToast(
            createNotice(
              'queue:restoredMissing',
              'warning',
              translate('composable.notices.restoreSkippedTitle'),
              translate('composable.notices.restoreSkippedBody', { count: missingCount }),
            ),
          )
        }

        if (!stillPresent.length) {
          return
        }
        entries = stillPresent
      } catch {
        // Existence probe unavailable — restore everything and let the
        // analysis pass surface missing files.
      }
    }

    addSourcePaths(entries.map((entry) => entry.sourcePath))
    for (const entry of entries) {
      const job = jobs.value.find(
        (item) => item.sourcePath.toLowerCase() === entry.sourcePath.toLowerCase(),
      )
      if (job) {
        job.settings = normalizeSettings({ ...entry.settings })
        job.settingsRestored = true
      }
    }
  }

  return {
    jobs,
    errorToasts,
    selectedJobId,
    sourceFileName,
    settings,
    analysis,
    result,
    nativeAvailable,
    analysisLoading,
    compressionLoading,
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
    dismissErrorToast,
    pauseErrorToast,
    resumeErrorToast,
    reportError,
    selectOutputDir,
    openCompressedFile,
    openCompressedFileFolder,
  }
}
