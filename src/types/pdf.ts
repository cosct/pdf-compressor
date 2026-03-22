export type CompressionPreset = 'conservative' | 'balanced' | 'maximum'
export type DocumentKind = 'text-native' | 'mixed' | 'scan-heavy'

export interface CompressionSettings {
  preset: CompressionPreset
  imageQuality: number
  maxImageSizePx: number
  optimizeImages: boolean
  compressStreams: boolean
  stripMetadata: boolean
}

export type NoticeTone = 'neutral' | 'success' | 'warning' | 'danger'

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
  streamsCompressed?: number
  metadataRemoved?: boolean
  outputWasSmaller?: boolean
  notes: NoticeItem[]
}

export type WorkflowState =
  | 'idle'
  | 'selected'
  | 'analyzing'
  | 'ready'
  | 'compressing'
  | 'success'
  | 'error'
