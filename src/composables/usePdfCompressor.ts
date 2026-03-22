import { computed, reactive, ref, watch } from 'vue'

import { i18n } from '../i18n'
import { analyzePdf, compressPdf, hasNativeCommands, openPdfDialog } from '../lib/tauri'
import type {
  AnalysisSummary,
  CompressionPreset,
  CompressionResult,
  CompressionSettings,
  DocumentKind,
  NoticeItem,
  NoticeTone,
  WorkflowState,
} from '../types/pdf'
import { fileNameFromPath } from '../utils/format'

function createDefaultSettings(): CompressionSettings {
  return {
    preset: 'balanced',
    imageQuality: 72,
    maxImageSizePx: 1800,
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
  }
}

function translate(key: string, values?: Record<string, unknown>): string {
  return values ? i18n.global.t(key, values) : i18n.global.t(key)
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

function createNotice(
  id: string,
  tone: NoticeTone,
  title: string,
  body: string,
): NoticeItem {
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

function normalizeAnalysis(input: unknown, sourcePath: string): AnalysisSummary {
  const record = asRecord(input) ?? {}
  const warnings = readStringArray(record, ['warnings', 'issues'])
  const messages = readStringArray(record, ['notes', 'recommendations', 'messages'])
  const scannedConfidence = readNumber(record, ['scannedConfidence', 'scanned_confidence'])
  const imageCoverage = readNumber(record, ['imageCoverage', 'image_coverage'])
  const estimatedSavingsPercent = readNumber(record, [
    'estimatedSavingsPercent',
    'estimated_savings_percent',
    'estimatedReductionPercent',
    'estimated_reduction_percent',
  ])
  const documentKind = normalizeDocumentKind(
    readString(record, ['documentKind', 'document_kind', 'kind']),
  )
  const isLikelyScanned = readBoolean(record, [
    'isLikelyScanned',
    'is_likely_scanned',
    'scanned',
    'imageBased',
    'image_based',
  ])

  // Accept both current and legacy backend key shapes so the frontend can keep
  // presenting a stable release UI while Rust payloads evolve.
  const notices: NoticeItem[] = [
    ...warnings.map((warning, index) =>
      createNotice(
        `warning-${index}`,
        'warning',
        translate('composable.notices.checkTitle'),
        warning,
      ),
    ),
    ...messages.map((message, index) =>
      createNotice(
        `note-${index}`,
        'neutral',
        translate('composable.notices.analysisNoteTitle'),
        message,
      ),
    ),
  ]

  if (isLikelyScanned === true) {
    notices.unshift(
      createNotice(
        'scanned-positive',
        'success',
        translate('composable.notices.goodCandidateTitle'),
        translate('composable.notices.goodCandidateBody'),
      ),
    )
  }

  if (isLikelyScanned === false) {
    notices.unshift(
      createNotice(
        'scanned-negative',
        'warning',
        translate('composable.notices.lowerSavingsTitle'),
        translate('composable.notices.lowerSavingsBody'),
      ),
    )
  }

  return {
    sourcePath,
    fileName: fileNameFromPath(sourcePath),
    fileSizeBytes: readNumber(record, ['fileSizeBytes', 'file_size_bytes', 'inputSizeBytes', 'sizeBytes']),
    pageCount: readNumber(record, ['pageCount', 'page_count', 'pages']),
    imageObjectCount: readNumber(record, ['imageObjectCount', 'image_object_count']),
    documentKind,
    imageCoverage,
    scannedConfidence,
    estimatedSavingsPercent,
    recommendedPreset: normalizePreset(
      readString(record, ['recommendedPreset', 'recommended_preset', 'preset']),
    ),
    isLikelyScanned,
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

  const warnings = readStringArray(record, ['warnings', 'issues'])
  const messages = readStringArray(record, ['notes', 'messages'])
  const outputWasSmaller = readBoolean(record, ['outputWasSmaller', 'output_was_smaller'])
  const notices: NoticeItem[] = [
    createNotice(
      'success',
      'success',
      translate('composable.notices.finishedTitle'),
      translate('composable.notices.finishedBody'),
    ),
    ...warnings.map((warning, index) =>
      createNotice(
        `warning-${index}`,
        'warning',
        translate('composable.notices.followUpTitle'),
        warning,
      ),
    ),
    ...messages.map((message, index) =>
      createNotice(
        `note-${index}`,
        'neutral',
        translate('composable.notices.backendNoteTitle'),
        message,
      ),
    ),
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
    outputWasSmaller,
    notes: dedupeNotices(notices),
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message
  }

  return translate('composable.errors.backendFallback')
}

export function usePdfCompressor() {
  const sourcePath = ref('')
  const settings = reactive(createDefaultSettings())
  const analysis = ref<AnalysisSummary | null>(null)
  const result = ref<CompressionResult | null>(null)
  const analysisLoading = ref(false)
  const compressionLoading = ref(false)
  const analysisError = ref('')
  const compressionError = ref('')
  const lastAnalysisPayload = ref<unknown>(null)
  const lastResultPayload = ref<unknown>(null)

  const nativeAvailable = hasNativeCommands()

  const normalizedSourcePath = computed(() => sourcePath.value.trim())
  const isPdfPath = computed(() => normalizedSourcePath.value.toLowerCase().endsWith('.pdf'))
  const sourceFileName = computed(() =>
    normalizedSourcePath.value ? fileNameFromPath(normalizedSourcePath.value) : '',
  )
  const sourcePathState = computed(() => {
    if (!normalizedSourcePath.value) {
      return {
        tone: 'neutral' as const,
        message: translate('composable.pathState.empty'),
      }
    }

    if (!isPdfPath.value) {
      return {
        tone: 'warning' as const,
        message: translate('composable.pathState.invalid'),
      }
    }

    return {
      tone: 'success' as const,
      message: translate('composable.pathState.ready'),
    }
  })
  const canAnalyze = computed(() => isPdfPath.value && !analysisLoading.value)
  const hasCompletedAnalysis = computed(
    () => analysis.value !== null && analysis.value.sourcePath === normalizedSourcePath.value,
  )
  // Compression stays locked to the most recently analyzed path so changing the
  // source immediately invalidates any stale analysis or result state.
  const canCompress = computed(
    () => isPdfPath.value && hasCompletedAnalysis.value && !analysisLoading.value && !compressionLoading.value,
  )
  const workflowState = computed<WorkflowState>(() => {
    if (analysisLoading.value) {
      return 'analyzing'
    }
    if (compressionLoading.value) {
      return 'compressing'
    }
    if (compressionError.value || analysisError.value) {
      return 'error'
    }
    if (result.value) {
      return 'success'
    }
    if (analysis.value) {
      return 'ready'
    }
    if (normalizedSourcePath.value) {
      return 'selected'
    }
    return 'idle'
  })

  watch(normalizedSourcePath, (current, previous) => {
    if (current === previous) {
      return
    }

    // Reset derived backend state whenever the path changes so the app never
    // compresses using analysis that belongs to a different file.
    analysis.value = null
    result.value = null
    analysisError.value = ''
    compressionError.value = ''
    lastAnalysisPayload.value = null
    lastResultPayload.value = null
  })

  watch(
    () => i18n.global.locale.value,
    () => {
      if (lastAnalysisPayload.value && normalizedSourcePath.value) {
        analysis.value = normalizeAnalysis(lastAnalysisPayload.value, normalizedSourcePath.value)
      }

      if (lastResultPayload.value && normalizedSourcePath.value) {
        result.value = normalizeCompressionResult(lastResultPayload.value, normalizedSourcePath.value)
      }
    },
  )

  function setSourcePath(path: string) {
    sourcePath.value = path
  }

  function updateSettings(next: CompressionSettings) {
    settings.preset = next.preset
    settings.imageQuality = next.imageQuality
    settings.maxImageSizePx = next.maxImageSizePx
    settings.optimizeImages = next.optimizeImages
    settings.compressStreams = next.compressStreams
    settings.stripMetadata = next.stripMetadata
  }

  async function browseForPdf() {
    analysisError.value = ''

    if (!nativeAvailable) {
      analysisError.value = translate('composable.errors.browseRequiresDesktop')
      return
    }

    try {
      const path = await openPdfDialog()
      if (!path) {
        return
      }

      sourcePath.value = path
    } catch (error) {
      analysisError.value = errorMessage(error)
    }
  }

  async function analyzeCurrentPdf() {
    if (!canAnalyze.value) {
      analysisError.value = translate('composable.errors.analyzeRequiresPdf')
      return
    }

    analysisLoading.value = true
    analysisError.value = ''
    result.value = null
    lastResultPayload.value = null

    try {
      const response = await analyzePdf(normalizedSourcePath.value)
      lastAnalysisPayload.value = response
      analysis.value = normalizeAnalysis(response, normalizedSourcePath.value)
    } catch (error) {
      analysisError.value = errorMessage(error)
      analysis.value = null
      lastAnalysisPayload.value = null
    } finally {
      analysisLoading.value = false
    }
  }

  async function compressCurrentPdf() {
    if (!canCompress.value) {
      compressionError.value = isPdfPath.value
        ? translate('composable.errors.compressRequiresAnalysis')
        : translate('composable.errors.compressRequiresPdf')
      return
    }

    compressionLoading.value = true
    compressionError.value = ''

    try {
      const response = await compressPdf(normalizedSourcePath.value, { ...settings })
      lastResultPayload.value = response
      result.value = normalizeCompressionResult(response, normalizedSourcePath.value)
    } catch (error) {
      compressionError.value = errorMessage(error)
      result.value = null
      lastResultPayload.value = null
    } finally {
      compressionLoading.value = false
    }
  }

  return {
    sourcePath,
    sourceFileName,
    settings,
    analysis,
    result,
    nativeAvailable,
    sourcePathState,
    analysisLoading,
    compressionLoading,
    analysisError,
    compressionError,
    workflowState,
    canAnalyze,
    canCompress,
    hasCompletedAnalysis,
    setSourcePath,
    updateSettings,
    browseForPdf,
    analyzeCurrentPdf,
    compressCurrentPdf,
  }
}
