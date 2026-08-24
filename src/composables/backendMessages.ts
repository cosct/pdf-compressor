/**
 * Backend message adapter — sanitizes backend payloads and localizes them.
 * 后端消息适配层 — 净化后端载荷并进行本地化。
 *
 * IPC types come from tauri-specta bindings; this module only adds runtime
 * tone/phase validation and i18n presentation on top.
 * IPC 类型来自 tauri-specta 生成绑定；本模块只做运行时 tone/phase 校验与 i18n 呈现。
 */
import type {
  AnalysisResponse,
  AppErrorPayload,
  BackendNotice,
  CompressionResponse,
} from '../lib/bindings'
import { i18n, translate } from '../i18n'
import type {
  AnalysisSummary,
  BackendMessage,
  CompressionResult,
  NoticeItem,
  NoticeTone,
} from '../types/pdf'
import { fileNameFromPath } from '../utils/format'

function formatTemplate(template: string, values: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? `{${key}}`)
}

export function clampPercent(value: number): number {
  if (!Number.isFinite(value)) {
    return 0
  }
  return Math.max(0, Math.min(100, value))
}

// ---------------------------------------------------------------------------
// Notice validation & localization
// ---------------------------------------------------------------------------

const NOTICE_TONES: readonly NoticeTone[] = ['neutral', 'success', 'warning', 'danger']

function normalizeTone(value: string | undefined, fallback: NoticeTone): NoticeTone {
  return NOTICE_TONES.includes(value as NoticeTone) ? (value as NoticeTone) : fallback
}

export function createNotice(id: string, tone: NoticeTone, title: string, body: string): NoticeItem {
  return { id, tone, title, body }
}

export function normalizeMessage(input: BackendNotice, fallbackLevel: NoticeTone = 'neutral'): BackendMessage {
  const values = input.values
    ? Object.fromEntries(
        Object.entries(input.values)
          .filter(([, value]) => ['string', 'number', 'boolean'].includes(typeof value))
          .map(([key, value]) => [key, String(value)]),
      )
    : undefined

  return {
    code: input.code || 'backend.unknown',
    level: normalizeTone(input.level, fallbackLevel),
    values,
    fallback: input.fallback,
  }
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

export function localizeNotices(notices: BackendNotice[], fallbackLevel?: NoticeTone): NoticeItem[] {
  return dedupeNotices(
    notices
      .map((item) => normalizeMessage(item, fallbackLevel))
      .map((item) => localizeBackendMessage(item, 'backend')),
  )
}

// ---------------------------------------------------------------------------
// Typed response → domain summary mapping
// ---------------------------------------------------------------------------

function optional(value: number | null | undefined): number | undefined {
  return value ?? undefined
}

export function mapAnalysisSummary(response: AnalysisResponse, sourcePath: string): AnalysisSummary {
  return {
    sourcePath,
    fileName: fileNameFromPath(sourcePath),
    fileSizeBytes: optional(response.fileSizeBytes),
    pageCount: response.pageCount,
    imageObjectCount: response.imageObjectCount,
    documentKind: normalizeDocumentKind(response.documentKind),
    imageCoverage: optional(response.imageCoverage),
    scannedConfidence: optional(response.scannedConfidence),
    estimatedSavingsPercent: optional(response.estimatedSavingsPercent),
    recommendedPreset: normalizePreset(response.recommendedPreset),
    maxImageEdgePx: response.maxImageEdgePx,
    recommendedMaxImageSizePx: response.recommendedMaxImageSizePx,
    recommendedImageQuality: response.recommendedImageQuality,
    isLikelyScanned: response.isLikelyScanned,
    notes: localizeNotices(response.notices),
  }
}

export function mapCompressionResult(
  response: CompressionResponse,
  sourcePath: string,
): CompressionResult {
  return {
    inputPath: sourcePath,
    outputPath: response.outputPath,
    originalSizeBytes: optional(response.originalSizeBytes),
    compressedSizeBytes: optional(response.compressedSizeBytes),
    savedBytes: optional(response.savedBytes),
    savingsPercent: optional(response.savingsPercent),
    elapsedMs: response.elapsedMs,
    imagesRecompressed: response.imagesRecompressed,
    imagesSkipped: response.imagesSkipped,
    imagesDeduplicated: response.imagesDeduplicated,
    streamsCompressed: response.streamsCompressed,
    metadataRemoved: response.metadataRemoved,
    outputWasSmaller: response.outputWasSmaller,
    notes: localizeNotices(response.notices),
  }
}

// ---------------------------------------------------------------------------
// Error normalization
// ---------------------------------------------------------------------------

export function normalizeError(error: unknown): NoticeItem {
  const payload = asErrorPayload(error)
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

function asErrorPayload(error: unknown): AppErrorPayload | null {
  if (error === null || typeof error !== 'object') {
    return null
  }
  const candidate = error as Partial<AppErrorPayload>
  if (typeof candidate.code !== 'string') {
    return null
  }
  return candidate as AppErrorPayload
}

export function isCancellationError(error: unknown): boolean {
  const payload = error === null || typeof error !== 'object' ? null : (error as AppErrorPayload)
  if (payload?.code === 'error.cancelled') {
    return true
  }

  const fallback = typeof payload?.fallback === 'string' ? payload.fallback : undefined
  return Boolean(fallback?.toLowerCase().includes('cancel'))
}

// ---------------------------------------------------------------------------
// Wire enum-ish string normalization
// ---------------------------------------------------------------------------

function normalizePreset(value: string): AnalysisSummary['recommendedPreset'] {
  const normalized = value.toLowerCase()

  if (normalized.includes('max') || normalized.includes('aggressive') || normalized.includes('strong')) {
    return 'maximum'
  }

  if (normalized.includes('light') || normalized.includes('gentle') || normalized.includes('conservative')) {
    return 'conservative'
  }

  return 'balanced'
}

function normalizeDocumentKind(value: string): AnalysisSummary['documentKind'] {
  const normalized = value.toLowerCase()

  if (normalized.includes('text')) {
    return 'text-native'
  }

  if (normalized.includes('scan')) {
    return 'scan-heavy'
  }

  return 'mixed'
}
