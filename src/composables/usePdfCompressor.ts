/**
 * Core workflow composable — single source of truth for the PDF compression pipeline.
 * 核心工作流 composable — PDF 压缩流水线的唯一状态来源。
 *
 * Manages the job queue, per-job settings, analysis results, compression results,
 * loading/error states, and concurrent compression scheduling.
 * 管理任务队列、逐任务设置、分析结果、压缩结果、加载/错误状态和并发压缩调度。
 *
 * Enforces the analyze-first-then-compress rule: each file must finish analysis
 * before its compression task is dispatched.
 * 强制执行"先分析再压缩"规则：每个文件必须先完成分析才能开始压缩。
 */
import { computed, ref } from 'vue'

import { getPresetDefaults } from '../config/presets'
import { i18n } from '../i18n'
import {
  analyzePdf,
  cancelCompression,
  compressPdf,
  hasNativeCommands,
  openDirectoryDialog,
  openPath,
  openPdfDialog,
  revealPathInFolder,
} from '../lib/tauri'
import type {
  AnalysisSummary,
  BackendMessage,
  CompressionPreset,
  CompressionResult,
  CompressionSettings,
  DocumentKind,
  NoticeItem,
  NoticeTone,
  PdfQueueJob,
  ProgressUpdate,
  WorkflowState,
} from '../types/pdf'
import {
  clampImageQuality,
  clampMaxImageSizePercent,
  normalizeReferenceMaxImageEdgePx,
} from '../utils/compressionSettings'
import { fileNameFromPath } from '../utils/format'

type ErrorPayload = {
  code?: string
  values?: Record<string, string>
  fallback?: string
  message?: string
}

function translate(key: string, values?: Record<string, unknown>): string {
  return values ? i18n.global.t(key, values) : i18n.global.t(key)
}

function formatTemplate(template: string, values: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? `{${key}}`)
}

function createSettingsForPreset(
  preset: CompressionPreset,
  referenceMaxImageEdgePx?: number | null,
  overrides?: Partial<Omit<CompressionSettings, 'preset'>>,
): CompressionSettings {
  const presetDefaults = getPresetDefaults(preset, referenceMaxImageEdgePx ?? undefined)
  const normalizedReferenceMaxImageEdgePx = normalizeReferenceMaxImageEdgePx(
    overrides?.referenceMaxImageEdgePx ?? referenceMaxImageEdgePx,
  )

  return normalizeSettings({
    preset,
    imageQuality: overrides?.imageQuality ?? presetDefaults.imageQuality,
    maxImageSizePercent: overrides?.maxImageSizePercent ?? presetDefaults.maxImageSizePercent,
    referenceMaxImageEdgePx: normalizedReferenceMaxImageEdgePx,
    optimizeImages: overrides?.optimizeImages ?? true,
    compressStreams: overrides?.compressStreams ?? true,
    stripMetadata: overrides?.stripMetadata ?? true,
    outputDir: overrides?.outputDir ?? null,
  })
}

function createDefaultSettings(): CompressionSettings {
  return createSettingsForPreset('balanced')
}

function clampPercent(value: number): number {
  return Math.max(0, Math.min(100, value))
}

function normalizeImageQuality(value: number | undefined, preset: CompressionPreset): number {
  const fallback = getPresetDefaults(preset).imageQuality
  const candidate = Number.isFinite(value) ? (value as number) : fallback
  return clampImageQuality(candidate)
}

function normalizeMaxImageSizePercent(value: number | undefined, preset: CompressionPreset): number {
  const fallback = getPresetDefaults(preset).maxImageSizePercent
  const candidate = Number.isFinite(value) ? (value as number) : fallback
  return clampMaxImageSizePercent(candidate)
}

function normalizeSettings(settings: CompressionSettings): CompressionSettings {
  const preset = settings.preset ?? 'balanced'

  return {
    ...settings,
    preset,
    imageQuality: normalizeImageQuality(settings.imageQuality, preset),
    maxImageSizePercent: normalizeMaxImageSizePercent(settings.maxImageSizePercent, preset),
    referenceMaxImageEdgePx: normalizeReferenceMaxImageEdgePx(settings.referenceMaxImageEdgePx),
    outputDir: settings.outputDir?.trim() ? settings.outputDir.trim() : null,
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' ? (value as Record<string, unknown>) : null
}

function readString(record: Record<string, unknown>, keys: string[]): string | undefined {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'string' && value.trim()) {
      return value
    }
  }

  return undefined
}

function readNumber(record: Record<string, unknown>, keys: string[]): number | undefined {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value
    }

    if (typeof value === 'string' && value.trim()) {
      const parsed = Number(value)
      if (Number.isFinite(parsed)) {
        return parsed
      }
    }
  }

  return undefined
}

function readBoolean(record: Record<string, unknown>, keys: string[]): boolean | undefined {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'boolean') {
      return value
    }
  }

  return undefined
}

function readStringArray(record: Record<string, unknown>, keys: string[]): string[] {
  for (const key of keys) {
    const value = record[key]
    if (Array.isArray(value)) {
      return value.filter(
        (entry): entry is string => typeof entry === 'string' && entry.trim().length > 0,
      )
    }
  }

  return []
}

function normalizePreset(value?: string): CompressionPreset | null {
  if (!value) {
    return null
  }

  const normalized = value.toLowerCase()

  if (normalized.includes('max') || normalized.includes('aggressive') || normalized.includes('strong')) {
    return 'maximum'
  }

  if (normalized.includes('light') || normalized.includes('gentle') || normalized.includes('conservative')) {
    return 'conservative'
  }

  return 'balanced'
}

function normalizeDocumentKind(value?: string): DocumentKind | null {
  if (!value) {
    return null
  }

  const normalized = value.toLowerCase()

  if (normalized.includes('text')) {
    return 'text-native'
  }

  if (normalized.includes('scan')) {
    return 'scan-heavy'
  }

  return 'mixed'
}

function normalizeMessage(input: unknown, fallbackLevel: NoticeTone = 'neutral'): BackendMessage | null {
  const record = asRecord(input)
  if (!record) {
    return null
  }

  const code = readString(record, ['code'])
  const fallback = readString(record, ['fallback', 'message'])
  if (!code && !fallback) {
    return null
  }

  const level = (readString(record, ['level', 'severity']) as NoticeTone | undefined) ?? fallbackLevel
  const valuesRecord = asRecord(record.values)
  const values = valuesRecord
    ? Object.fromEntries(
        Object.entries(valuesRecord)
          .filter(([, value]) => ['string', 'number', 'boolean'].includes(typeof value))
          .map(([key, value]) => [key, String(value)]),
      )
    : undefined

  return {
    code: code ?? 'backend.unknown',
    level,
    values,
    fallback,
  }
}

function createNotice(id: string, tone: NoticeTone, title: string, body: string): NoticeItem {
  return { id, tone, title, body }
}

function dedupeNotices(notices: NoticeItem[]): NoticeItem[] {
  const seen = new Set<string>()
  return notices.filter((notice) => {
    const key = `${notice.tone}:${notice.title}:${notice.body}`
    if (seen.has(key)) {
      return false
    }
    seen.add(key)
    return true
  })
}

function localizeBackendMessage(message: BackendMessage, prefix: 'backend' | 'error'): NoticeItem {
  const values = message.values ?? {}
  const bucket = i18n.global.tm(prefix) as Record<string, { title?: string; body?: string }>
  const entry = bucket?.[message.code]

  const title = entry?.title
    ? formatTemplate(entry.title, values)
    : prefix === 'error'
      ? translate('app.alertTitle')
      : translate('composable.notices.backendNoteTitle')
  const body = entry?.body ? formatTemplate(entry.body, values) : message.fallback ?? message.code

  return createNotice(`${message.code}:${JSON.stringify(values)}`, message.level, title, body)
}

function normalizeLegacyMessages(messages: string[], tone: NoticeTone, titleKey: string): NoticeItem[] {
  return messages.map((message, index) =>
    createNotice(`${tone}-${index}`, tone, translate(titleKey), message),
  )
}

function normalizeAnalysis(input: unknown, sourcePath: string): AnalysisSummary {
  const record = asRecord(input) ?? {}
  const notices = Array.isArray(record.notices)
    ? record.notices
        .map((item) => normalizeMessage(item))
        .filter((item): item is BackendMessage => item !== null)
        .map((item) => localizeBackendMessage(item, 'backend'))
    : [
        ...normalizeLegacyMessages(readStringArray(record, ['warnings', 'issues']), 'warning', 'composable.notices.checkTitle'),
        ...normalizeLegacyMessages(readStringArray(record, ['notes', 'recommendations', 'messages']), 'neutral', 'composable.notices.analysisNoteTitle'),
      ]

  return {
    sourcePath,
    fileName: fileNameFromPath(sourcePath),
    fileSizeBytes: readNumber(record, ['fileSizeBytes', 'file_size_bytes', 'inputSizeBytes', 'sizeBytes']),
    pageCount: readNumber(record, ['pageCount', 'page_count', 'pages']),
    imageObjectCount: readNumber(record, ['imageObjectCount', 'image_object_count']),
    documentKind: normalizeDocumentKind(readString(record, ['documentKind', 'document_kind', 'kind'])),
    imageCoverage: readNumber(record, ['imageCoverage', 'image_coverage']),
    scannedConfidence: readNumber(record, ['scannedConfidence', 'scanned_confidence']),
    estimatedSavingsPercent: readNumber(record, [
      'estimatedSavingsPercent',
      'estimated_savings_percent',
      'estimatedReductionPercent',
      'estimated_reduction_percent',
    ]),
    recommendedPreset: normalizePreset(readString(record, ['recommendedPreset', 'recommended_preset', 'preset'])),
    maxImageEdgePx: readNumber(record, ['maxImageEdgePx', 'max_image_edge_px']),
    recommendedMaxImageSizePx: readNumber(record, ['recommendedMaxImageSizePx', 'recommended_max_image_size_px']),
    recommendedImageQuality: readNumber(record, ['recommendedImageQuality', 'recommended_image_quality']),
    isLikelyScanned: readBoolean(record, ['isLikelyScanned', 'is_likely_scanned', 'scanned']),
    notes: dedupeNotices(notices),
  }
}

function normalizeCompressionResult(input: unknown, sourcePath: string): CompressionResult {
  const record = asRecord(input) ?? {}
  const originalSizeBytes = readNumber(record, [
    'originalSizeBytes',
    'original_size_bytes',
    'inputSizeBytes',
    'input_size_bytes',
  ])
  const compressedSizeBytes = readNumber(record, [
    'compressedSizeBytes',
    'compressed_size_bytes',
    'outputSizeBytes',
    'output_size_bytes',
  ])
  const savedBytes =
    readNumber(record, ['savedBytes', 'saved_bytes', 'bytesSaved']) ??
    (originalSizeBytes !== undefined && compressedSizeBytes !== undefined
      ? originalSizeBytes - compressedSizeBytes
      : undefined)
  const savingsPercent =
    readNumber(record, ['savingsPercent', 'savings_percent', 'reductionPercent']) ??
    (savedBytes !== undefined && originalSizeBytes && originalSizeBytes > 0
      ? (savedBytes / originalSizeBytes) * 100
      : undefined)

  const notices = Array.isArray(record.notices)
    ? record.notices
        .map((item) => normalizeMessage(item))
        .filter((item): item is BackendMessage => item !== null)
        .map((item) => localizeBackendMessage(item, 'backend'))
    : [
        ...normalizeLegacyMessages(readStringArray(record, ['warnings', 'issues']), 'warning', 'composable.notices.followUpTitle'),
        ...normalizeLegacyMessages(readStringArray(record, ['notes', 'messages']), 'neutral', 'composable.notices.backendNoteTitle'),
      ]

  return {
    inputPath: sourcePath,
    outputPath:
      readString(record, ['outputPath', 'output_path', 'destinationPath', 'destination_path']) ??
      sourcePath,
    originalSizeBytes,
    compressedSizeBytes,
    savedBytes,
    savingsPercent,
    elapsedMs: readNumber(record, ['elapsedMs', 'elapsed_ms', 'durationMs', 'duration_ms']),
    imagesRecompressed: readNumber(record, ['imagesRecompressed', 'images_recompressed']),
    imagesSkipped: readNumber(record, ['imagesSkipped', 'images_skipped']),
    streamsCompressed: readNumber(record, ['streamsCompressed', 'streams_compressed']),
    metadataRemoved: readBoolean(record, ['metadataRemoved', 'metadata_removed']),
    outputWasSmaller: readBoolean(record, ['outputWasSmaller', 'output_was_smaller']),
    notes: dedupeNotices(notices),
  }
}

function normalizeError(error: unknown): NoticeItem {
  const payload = asRecord(error) as ErrorPayload | null
  if (payload?.code) {
    return localizeBackendMessage(
      {
        code: payload.code,
        level: 'danger',
        values: payload.values,
        fallback: payload.fallback,
      },
      'error',
    )
  }

  if (error instanceof Error && error.message) {
    return createNotice('error:fallback', 'danger', translate('app.alertTitle'), error.message)
  }

  return createNotice(
    'error:unknown',
    'danger',
    translate('app.alertTitle'),
    translate('composable.errors.backendFallback'),
  )
}

function isCancellationError(error: unknown): boolean {
  const payload = asRecord(error)
  const code = payload ? readString(payload, ['code']) : undefined
  if (code === 'error.cancelled') {
    return true
  }

  const fallback = payload ? readString(payload, ['fallback', 'message']) : undefined
  return Boolean(fallback?.toLowerCase().includes('cancel'))
}

function isPdfPath(path: string): boolean {
  return path.trim().toLowerCase().endsWith('.pdf')
}

function mapProgressPhaseToWorkflow(phase: ProgressUpdate['phase']): WorkflowState {
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
  const defaults = createDefaultSettings()

  return {
    id: `${normalizedPath}::${Date.now()}::${Math.random().toString(36).slice(2, 8)}`,
    sourcePath: normalizedPath,
    fileName: fileNameFromPath(normalizedPath),
    status: normalizedPath ? 'selected' : 'idle',
    progress: {
      phase: 'queued',
      percent: 0,
    },
    analysis: null,
    result: null,
    settings: { ...defaults },
    recommendedSettings: { ...defaults },
    useRecommendedSettings: true,
    error: null,
    lastAction: null,
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
  }
}

function getCompressionConcurrency(jobCount: number): number {
  const cpu = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 4 : 4
  if (jobCount <= 1) {
    return 1
  }

  return Math.min(2, Math.max(1, Math.floor(cpu / 2)))
}

export function usePdfCompressor() {
  const jobs = ref<PdfQueueJob[]>([])
  const errorToasts = ref<NoticeItem[]>([])
  const selectedJobId = ref<string | null>(null)
  const draftSettings = ref(createDefaultSettings())
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
  const analysisError = computed(() =>
    selectedJob.value?.lastAction === 'analyze' ? selectedJob.value.error?.body ?? '' : '',
  )
  const compressionError = computed(() =>
    selectedJob.value?.lastAction === 'compress' ? selectedJob.value.error?.body ?? '' : '',
  )
  const workflowState = computed<WorkflowState>(() => selectedJob.value?.status ?? 'idle')
  const jobsPendingCompression = computed(() =>
    jobs.value.filter(
      (job) =>
        isPdfPath(job.sourcePath) &&
        job.status !== 'compressing' &&
        job.status !== 'success',
    ),
  )
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
  const canCompressSelected = computed(
    () => !compressionRunning.value && selectedCompressionTargetIds.value.length > 0,
  )
  const canCompressAll = computed(
    () => !compressionRunning.value && allCompressionTargetIds.value.length > 0,
  )
  const canCancelCompression = computed(() => compressionRunning.value)

  function pushErrorToast(notice: NoticeItem) {
    const duplicate = errorToasts.value.some(
      (item) =>
        item.tone === notice.tone &&
        item.title === notice.title &&
        item.body === notice.body,
    )
    if (duplicate) {
      return
    }

    errorToasts.value = [
      ...errorToasts.value,
      {
        ...notice,
        id: `${notice.id}:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
      },
    ]
  }

  function dismissErrorToast(id: string) {
    errorToasts.value = errorToasts.value.filter((item) => item.id !== id)
  }

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

  function setJobStatus(job: PdfQueueJob, status: WorkflowState) {
    job.status = status
  }

  function applyProgress(job: PdfQueueJob, update: ProgressUpdate) {
    job.progress = {
      phase: update.phase,
      percent: clampPercent(update.percent),
      message: update.message ?? null,
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
    job.recommendedSettings = {
      ...createRecommendedSettings(job.analysis),
      outputDir,
    }
    job.settings = { ...job.recommendedSettings }
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
      pushErrorToast(normalizeError(error))
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
    job.lastAction = 'analyze'
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

      job.analysis = normalizeAnalysis(response, requestedPath)
      applyRecommendedSettings(job)
      job.progress = { phase: 'done', percent: 100 }
      setJobStatus(job, 'ready')
    } catch (error) {
      if (job.sourcePath !== requestedPath) {
        return
      }

      job.error = normalizeError(error)
      pushErrorToast(job.error)
      job.progress = { phase: 'error', percent: 100 }
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
    job.lastAction = 'compress'
    job.error = null
    job.result = null
    activeCompressionTaskIds.set(job.id, taskId)
    applyProgress(job, { phase: 'compressing', percent: 0 })

    try {
      const response = await compressPdf(requestedPath, job.settings, taskId, (update) => {
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

      job.result = normalizeCompressionResult(response, requestedPath)
      job.progress = { phase: 'done', percent: 100 }
      setJobStatus(job, 'success')
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
      job.progress = { phase: 'error', percent: 100 }
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

  async function compressSelectedPdf() {
    if (!canCompressSelected.value) {
      return
    }

    await runCompressionTargets(getSelectedCompressionTargetIds())
  }

  async function compressAllPdfs() {
    if (!canCompressAll.value) {
      return
    }

    await runCompressionTargets(getAllCompressionTargetIds())
  }

  async function compressCurrentPdf() {
    if (!canCompress.value) {
      return
    }

    await runCompressionTargets(getPrimaryCompressionTargetIds())
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
    } catch {
      // ignore dialog cancellation
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
      pushErrorToast(normalizeError(error))
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
      pushErrorToast(normalizeError(error))
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
    analysisError,
    compressionError,
    workflowState,
    canCompress,
    canCompressSelected,
    canCompressAll,
    canCancelCompression,
    updateSettings,
    applySettingsToAll,
    browseForPdf,
    addSourcePaths,
    selectJob,
    removeJobById,
    compressCurrentPdf,
    compressSelectedPdf,
    compressAllPdfs,
    cancelCompressionRun,
    dismissErrorToast,
    selectOutputDir,
    openCompressedFile,
    openCompressedFileFolder,
  }
}
