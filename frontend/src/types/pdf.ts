/**
 * Frontend type definitions for the PDF compression workflow.
 * PDF 压缩工作流的前端类型定义。
 */

export type CompressionPreset = 'conservative' | 'balanced' | 'maximum' | 'custom'
export type DocumentKind = 'text-native' | 'mixed' | 'scan-heavy'
export type NoticeTone = 'neutral' | 'success' | 'warning' | 'danger'
/** Output codec for near-black-and-white scanned images. */
export type BilevelCodec = 'jpeg' | 'ccitt-g4'
export type WorkflowState =
  | 'idle'
  | 'selected'
  | 'analyzing'
  | 'ready'
  | 'compressing'
  | 'success'
  | 'error'

/** Status a queue job can take — 'idle' is reserved for the empty-queue workflow state. */
export type QueueItemStatus = Exclude<WorkflowState, 'idle'>

export type ProgressPhase = 'queued' | 'analyzing' | 'compressing' | 'writing' | 'done' | 'error'

export interface CompressionSettings {
  preset: CompressionPreset
  imageQuality: number
  maxImageSizePercent: number
  referenceMaxImageEdgePx: number | null
  optimizeImages: boolean
  compressStreams: boolean
  stripMetadata: boolean
  /** Re-encode color images as grayscale (best for black-and-white scans). */
  grayscale: boolean
  /** Codec used when the decoded plane is near-bilevel: `'ccitt-g4'` switches
   * those images to lossless CCITT Group 4 (text scans). */
  bilevelCodec: BilevelCodec
  /** Shrink embedded CID TrueType fonts to the used glyphs (opt-in). */
  subsetFonts: boolean
  outputDir: string | null
  /** Optional target output size in MB — the backend searches quality/edge
   * parameters until the output fits (best effort). `null` disables. */
  targetFileSizeMb: number | null
}

export interface PresetDefaults {
  imageQuality: number
  maxImageSizePercent: number
  optimizeImages: boolean
  compressStreams: boolean
  stripMetadata: boolean
  grayscale: boolean
  bilevelCodec: BilevelCodec
  subsetFonts: boolean
}

export type PresetProfile = PresetDefaults

export type PresetProfileMap = Record<CompressionPreset, PresetProfile>

export interface PresetUserConfig {
  version: number
  presets: Partial<Record<CompressionPreset, PresetProfile>>
}

export interface BackendMessage {
  code: string
  level: NoticeTone
  values?: Record<string, string>
  fallback?: string
}

export interface ProgressUpdate {
  phase: ProgressPhase
  percent: number
}

export interface NoticeItem {
  id: string
  tone: NoticeTone
  title: string
  body: string
}

export interface AnalysisSummary {
  sourcePath: string
  fileName: string
  fileSizeBytes?: number
  pageCount?: number
  imageObjectCount?: number
  documentKind?: DocumentKind | null
  imageCoverage?: number | null
  scannedConfidence?: number | null
  estimatedSavingsPercent?: number | null
  recommendedPreset?: CompressionPreset | null
  maxImageEdgePx?: number | null
  recommendedMaxImageSizePx?: number | null
  recommendedImageQuality?: number | null
  isLikelyScanned?: boolean | null
  notes: NoticeItem[]
}

export interface CompressionResult {
  inputPath: string
  outputPath: string
  originalSizeBytes?: number
  compressedSizeBytes?: number
  savedBytes?: number
  savingsPercent?: number
  elapsedMs?: number
  imagesRecompressed?: number
  imagesSkipped?: number
  imagesDeduplicated?: number
  streamsCompressed?: number
  metadataRemoved?: boolean
  outputWasSmaller?: boolean
  notes: NoticeItem[]
}

export interface PdfQueueJob {
  id: string
  sourcePath: string
  fileName: string
  status: QueueItemStatus
  progress: ProgressUpdate
  analysis: AnalysisSummary | null
  result: CompressionResult | null
  settings: CompressionSettings
  recommendedSettings: CompressionSettings
  useRecommendedSettings: boolean
  /** Settings came from the persisted queue — the first analysis pass after
   *  a restart must not overwrite them with the fresh recommendation. */
  settingsRestored: boolean
  /** Open password for encrypted PDFs, entered per job via the retry prompt.
   *  Session-only by design — never written to the persisted queue. */
  password: string | null
  error: NoticeItem | null
}

/** Backend error codes that mean "this file needs (a better) password". */
export const PASSWORD_ERROR_CODES = ['error.passwordRequired', 'error.wrongPassword'] as const

export function isPasswordError(error: NoticeItem | null): boolean {
  if (error === null) {
    return false
  }
  // Backend codes live as prefixes of the notice id (`code:{values}`); the
  // notice itself does not carry the code as a dedicated field.
  return PASSWORD_ERROR_CODES.some((code) => error.id === code || error.id.startsWith(`${code}:`))
}

export function isWrongPasswordError(error: NoticeItem | null): boolean {
  return (
    error !== null &&
    (error.id === 'error.wrongPassword' || error.id.startsWith('error.wrongPassword:'))
  )
}
