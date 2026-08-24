/**
 * Frontend type definitions for the PDF compression workflow.
 * PDF 压缩工作流的前端类型定义。
 */

export type CompressionPreset = 'conservative' | 'balanced' | 'maximum' | 'custom'
export type DocumentKind = 'text-native' | 'mixed' | 'scan-heavy'
export type NoticeTone = 'neutral' | 'success' | 'warning' | 'danger'
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
  outputDir: string | null
  /** Optional target output size in MB — the backend searches quality/edge
   * parameters until the output fits (best effort). `null` disables. */
  targetFileSizeMb: number | null
}

export interface PresetDefaults {
  imageQuality: number
  maxImageSizePercent: number
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
  error: NoticeItem | null
}
