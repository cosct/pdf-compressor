/**
 * Scheduler / queue / cancellation tests for the core workflow composable.
 * 核心工作流 composable 的调度/队列/取消测试。
 *
 * The Tauri bridge (`src/lib/tauri.ts`) is fully mocked; these tests cover
 * queue-state transitions only, not backend behavior.
 * Tauri 桥接层完全 mock；只覆盖队列状态转换，不覆盖后端行为。
 */
import { beforeEach, describe, expect, it, vi } from 'vite-plus/test'

import type { AnalysisResponse, CompressionResponse } from '../../lib/bindings'
import type { CompressionSettings } from '../../types/pdf'

vi.mock('../../lib/tauri', () => ({
  hasNativeCommands: () => true,
  analyzePdf: vi.fn(),
  compressPdf: vi.fn(),
  compressScannedPdf: vi.fn(),
  cancelCompression: vi.fn().mockResolvedValue(undefined),
  existingPaths: vi.fn().mockResolvedValue([]),
  openDirectoryDialog: vi.fn(),
  openPath: vi.fn(),
  openPdfDialog: vi.fn(),
  revealPathInFolder: vi.fn(),
}))

import {
  analyzePdf,
  compressPdf,
  compressScannedPdf,
  cancelCompression,
  existingPaths,
} from '../../lib/tauri'
import { getCompressionConcurrency, normalizeSettings, usePdfCompressor } from '../usePdfCompressor'
import { localizeNotices } from '../backendMessages'

function analysisResponse(overrides: Partial<AnalysisResponse> = {}): AnalysisResponse {
  return {
    fileSizeBytes: 1_000_000,
    pageCount: 1,
    imageObjectCount: 1,
    documentKind: 'mixed',
    scannedConfidence: 10,
    imageCoverage: 40,
    estimatedSavingsPercent: 20,
    recommendedPreset: 'balanced',
    maxImageEdgePx: 1600,
    recommendedMaxImageSizePx: 1800,
    recommendedImageQuality: 72,
    isLikelyScanned: false,
    notices: [],
    ...overrides,
  }
}

function compressionResponse(overrides: Partial<CompressionResponse> = {}): CompressionResponse {
  return {
    outputPath: '/tmp/out.pdf',
    originalSizeBytes: 1_000_000,
    compressedSizeBytes: 400_000,
    savedBytes: 600_000,
    savingsPercent: 60,
    elapsedMs: 123,
    imagesRecompressed: 1,
    imagesSkipped: 0,
    imagesDeduplicated: 0,
    streamsCompressed: 1,
    metadataRemoved: true,
    outputWasSmaller: true,
    notices: [],
    ...overrides,
  }
}

function makeSettings(overrides: Partial<CompressionSettings> = {}): CompressionSettings {
  return {
    preset: 'balanced',
    imageQuality: 72,
    maxImageSizePercent: 80,
    referenceMaxImageEdgePx: null,
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
    grayscale: false,
    bilevelCodec: 'jpeg',
    subsetFonts: false,
    outputDir: null,
    targetFileSizeMb: null,
    ...overrides,
  }
}

const mockedAnalyze = vi.mocked(analyzePdf)
const mockedCompress = vi.mocked(compressPdf)
const mockedCompressScanned = vi.mocked(compressScannedPdf)
const mockedCancel = vi.mocked(cancelCompression)

async function readyQueue(paths: string[]) {
  const composable = usePdfCompressor()
  mockedAnalyze.mockResolvedValue(analysisResponse())
  composable.addSourcePaths(paths)
  await vi.waitFor(() => {
    expect(composable.jobs.value.every((job) => job.status === 'ready')).toBe(true)
  })
  return composable
}

// happy-dom here ships no localStorage — the composable guards every access
// with try/catch, so a fresh in-memory stub per test exercises the real
// persistence paths (queue restore among them).
let localStorageStore: Map<string, string>

beforeEach(() => {
  vi.clearAllMocks()
  mockedCancel.mockResolvedValue(undefined)
  localStorageStore = new Map<string, string>()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      get length() {
        return localStorageStore.size
      },
      clear: () => localStorageStore.clear(),
      getItem: (key: string) => localStorageStore.get(key) ?? null,
      key: (index: number) => [...localStorageStore.keys()][index] ?? null,
      removeItem: (key: string) => {
        localStorageStore.delete(key)
      },
      setItem: (key: string, value: string) => {
        localStorageStore.set(key, String(value))
      },
    } satisfies Storage,
  })
})

describe('getCompressionConcurrency', () => {
  it('runs a single job serially', () => {
    expect(getCompressionConcurrency(1)).toBe(1)
  })

  it('caps parallel jobs at two regardless of cpu count', () => {
    const concurrency = getCompressionConcurrency(8)
    expect(concurrency).toBeGreaterThanOrEqual(1)
    expect(concurrency).toBeLessThanOrEqual(2)
  })
})

describe('normalizeSettings', () => {
  it('clamps quality and percent into range and trims output dir', () => {
    const normalized = normalizeSettings({
      preset: 'balanced',
      imageQuality: 500,
      maxImageSizePercent: -20,
      referenceMaxImageEdgePx: 1234,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: false,
      bilevelCodec: 'jpeg',
      subsetFonts: false,
      outputDir: '  /tmp/out  ',
      targetFileSizeMb: 99999,
    })
    expect(normalized.imageQuality).toBeLessThanOrEqual(100)
    expect(normalized.maxImageSizePercent).toBeGreaterThanOrEqual(1)
    expect(normalized.outputDir).toBe('/tmp/out')
    expect(normalized.targetFileSizeMb).toBeLessThanOrEqual(2048)
    expect(
      normalizeSettings({
        preset: 'balanced',
        imageQuality: 72,
        maxImageSizePercent: 80,
        referenceMaxImageEdgePx: 1234,
        optimizeImages: true,
        compressStreams: true,
        stripMetadata: true,
        grayscale: false,
        bilevelCodec: 'jpeg',
        subsetFonts: false,
        outputDir: null,
        targetFileSizeMb: 0,
      }).targetFileSizeMb,
    ).toBeNull()
  })

  it('coerces a missing grayscale field (legacy persisted queue) to false', () => {
    const normalized = normalizeSettings({
      preset: 'balanced',
      imageQuality: 72,
      maxImageSizePercent: 80,
      referenceMaxImageEdgePx: 1234,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: undefined as unknown as boolean,
      bilevelCodec: undefined as unknown as 'jpeg',
      subsetFonts: undefined as unknown as boolean,
      outputDir: null,
      targetFileSizeMb: null,
    })
    expect(normalized.grayscale).toBe(false)
    expect(normalized.bilevelCodec).toBe('jpeg')
  })

  it('keeps ccitt-g4 as the only non-default bilevel codec', () => {
    const normalized = normalizeSettings({
      preset: 'balanced',
      imageQuality: 72,
      maxImageSizePercent: 80,
      referenceMaxImageEdgePx: 1234,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: true,
      bilevelCodec: 'ccitt-g4',
      subsetFonts: true,
      outputDir: null,
      targetFileSizeMb: null,
    })
    expect(normalized.bilevelCodec).toBe('ccitt-g4')
    expect(normalized.subsetFonts).toBe(true)
  })
})

describe('addSourcePaths', () => {
  it('creates a job per pdf and marks it ready after analysis', async () => {
    const composable = await readyQueue(['/tmp/a.pdf', '/tmp/b.pdf'])

    expect(composable.jobs.value).toHaveLength(2)
    expect(mockedAnalyze).toHaveBeenCalledTimes(2)
    expect(composable.jobs.value.every((job) => job.analysis !== null)).toBe(true)
  })

  it('preserves a draft target size when analysis applies recommended settings', async () => {
    // With no file selected, edits land in the draft; a target size is a user
    // budget orthogonal to the recommended quality knobs, so applying the
    // recommendation on analysis completion must not drop it.
    const composable = usePdfCompressor()
    mockedAnalyze.mockResolvedValue(analysisResponse())
    composable.updateSettings(
      normalizeSettings({
        ...makeSettings(),
        targetFileSizeMb: 5,
      }),
    )

    composable.addSourcePaths(['/tmp/a.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('ready')
    })

    expect(composable.jobs.value[0].settings.targetFileSizeMb).toBe(5)
  })

  it('resets progress to 0% when analysis finishes — 100% means compressed', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])

    expect(composable.jobs.value[0].status).toBe('ready')
    expect(composable.jobs.value[0].progress.percent).toBe(0)

    // Only a completed compression run reaches 100%.
    mockedCompress.mockResolvedValue(compressionResponse())
    await composable.compressCurrentPdf()
    expect(composable.jobs.value[0].status).toBe('success')
    expect(composable.jobs.value[0].progress.percent).toBe(100)
  })

  it('keeps progress at 0% when analysis fails', async () => {
    mockedAnalyze.mockRejectedValue(new Error('boom'))
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/broken.pdf'])

    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('error')
    })
    expect(composable.jobs.value[0].progress.percent).toBe(0)
  })

  it('ignores duplicate paths case-insensitively', async () => {
    const composable = await readyQueue(['/tmp/A.pdf'])
    composable.addSourcePaths(['/tmp/a.pdf'])

    expect(composable.jobs.value).toHaveLength(1)
  })

  it('rejects additions while a compression run is active', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])

    let releaseCompress!: () => void
    mockedCompress.mockImplementation(
      () =>
        new Promise<CompressionResponse>((resolve) => {
          releaseCompress = () => resolve(compressionResponse())
        }),
    )

    const run = composable.compressCurrentPdf()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('compressing')
    })

    composable.addSourcePaths(['/tmp/late.pdf'])
    expect(composable.jobs.value).toHaveLength(1)
    expect(composable.errorToasts.value).toHaveLength(1)

    releaseCompress()
    await run
  })
})

describe('compressCurrentPdf', () => {
  it('stores the result and marks the job successful', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])
    mockedCompress.mockResolvedValue(compressionResponse({ outputPath: '/tmp/out.pdf' }))

    await composable.compressCurrentPdf()

    expect(composable.jobs.value[0].status).toBe('success')
    expect(composable.jobs.value[0].result?.outputPath).toBe('/tmp/out.pdf')
  })

  it('re-compresses a completed job once nothing is pending', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])
    mockedCompress.mockResolvedValue(compressionResponse())

    await composable.compressCurrentPdf()
    expect(composable.jobs.value[0].status).toBe('success')

    // Completion must not dead-end the queue: with no pending files left the
    // selected job itself becomes the target again.
    await composable.compressCurrentPdf()

    expect(mockedCompress).toHaveBeenCalledTimes(2)
    expect(composable.jobs.value[0].status).toBe('success')
  })

  it('routes scan-heavy documents through the scanned pipeline', async () => {
    mockedAnalyze.mockResolvedValue(
      analysisResponse({ documentKind: 'scan-heavy', isLikelyScanned: true }),
    )
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/scan.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value.every((job) => job.status === 'ready')).toBe(true)
    })
    mockedCompressScanned.mockResolvedValue(
      compressionResponse({ outputPath: '/tmp/scan-out.pdf' }),
    )

    await composable.compressCurrentPdf()

    expect(mockedCompressScanned).toHaveBeenCalledTimes(1)
    expect(mockedCompress).not.toHaveBeenCalled()
    expect(composable.jobs.value[0].status).toBe('success')
    expect(composable.errorToasts.value.some((toast) => toast.id.startsWith('scan:pipeline'))).toBe(
      true,
    )
  })

  it('surfaces compression errors as job error and toast', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])
    mockedCompress.mockRejectedValue({
      code: 'error.pdfBuild',
      values: { detail: 'broken' },
      fallback: 'Failed to build the output PDF: broken',
    })

    await composable.compressCurrentPdf()

    expect(composable.jobs.value[0].status).toBe('error')
    expect(composable.errorToasts.value).toHaveLength(1)
  })

  it('surfaces warning-level result notes as toasts', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])
    mockedCompress.mockResolvedValue(
      compressionResponse({
        notices: [
          {
            code: 'compress.warning.targetSizeMissed',
            level: 'warning',
            values: { targetKb: '512' },
            fallback: 'Could not reach the 512 KB target.',
          },
        ],
      }),
    )

    await composable.compressCurrentPdf()

    expect(composable.jobs.value[0].status).toBe('success')
    expect(composable.errorToasts.value.some((toast) => toast.tone === 'warning')).toBe(true)
  })
})

describe('compressSelectedPdf', () => {
  it('compresses only the selected job while others stay pending', async () => {
    const composable = await readyQueue(['/tmp/a.pdf', '/tmp/b.pdf'])
    composable.selectJob(composable.jobs.value[1].id)
    mockedCompress.mockImplementation((path: string) =>
      Promise.resolve(compressionResponse({ outputPath: `/tmp/out-${path}` })),
    )

    await composable.compressSelectedPdf()

    expect(mockedCompress).toHaveBeenCalledTimes(1)
    expect(composable.jobs.value[0].status).toBe('ready')
    expect(composable.jobs.value[1].status).toBe('success')
  })
})

describe('cancelCompressionRun', () => {
  it('resets the job, cancels the backend task, and drops the late result', async () => {
    const composable = await readyQueue(['/tmp/a.pdf'])

    let releaseCompress!: () => void
    mockedCompress.mockImplementation(
      () =>
        new Promise<CompressionResponse>((resolve) => {
          releaseCompress = () => resolve(compressionResponse())
        }),
    )

    const run = composable.compressCurrentPdf()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('compressing')
    })

    await composable.cancelCompressionRun()
    expect(composable.jobs.value[0].status).toBe('ready')
    expect(mockedCancel).toHaveBeenCalledTimes(1)

    // The run resolves successfully after cancellation — the result must be
    // discarded, not applied.
    releaseCompress()
    await run

    expect(composable.jobs.value[0].status).toBe('ready')
    expect(composable.jobs.value[0].result).toBeNull()
  })
})

describe('localizeNotices', () => {
  it('maps backend notice codes to localized bodies with values', () => {
    const notices = localizeNotices([
      {
        code: 'compress.warning.imageSkipped',
        level: 'warning',
        values: { objectId: '(5, 0)', reason: 'unsupported filter' },
        fallback: 'Skipped image object (5, 0): unsupported filter',
      },
    ])

    expect(notices).toHaveLength(1)
    expect(notices[0].tone).toBe('warning')
    expect(notices[0].body).toContain('(5, 0)')
    expect(notices[0].body).toContain('unsupported filter')
  })

  it('falls back to the backend text for unknown codes and invalid tones', () => {
    const notices = localizeNotices([
      {
        code: 'compress.note.unknownFutureCode',
        level: 'sparkly',
        values: {},
        fallback: 'Something new from the backend',
      },
    ])

    expect(notices[0].tone).toBe('neutral')
    expect(notices[0].body).toBe('Something new from the backend')
  })
})

describe('queue restore', () => {
  it('keeps restored per-job settings when the analysis lands', async () => {
    const persistedSettings = normalizeSettings({
      preset: 'maximum',
      imageQuality: 45,
      maxImageSizePercent: 55,
      referenceMaxImageEdgePx: null,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: true,
      bilevelCodec: 'ccitt-g4',
      subsetFonts: true,
      outputDir: '/tmp/custom-out',
      targetFileSizeMb: null,
    })
    window.localStorage.setItem(
      'pdf-compressor-queue',
      JSON.stringify([{ sourcePath: '/tmp/restored.pdf', settings: persistedSettings }]),
    )
    vi.mocked(existingPaths).mockResolvedValue(['/tmp/restored.pdf'])
    mockedAnalyze.mockResolvedValue(analysisResponse({ recommendedPreset: 'conservative' }))

    const composable = usePdfCompressor()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0]?.status).toBe('ready')
    })

    // The analysis recommendation must not clobber the restored settings.
    const job = composable.jobs.value[0]
    expect(job.analysis?.recommendedPreset).toBe('conservative')
    expect(job.settings.preset).toBe('maximum')
    expect(job.settings.imageQuality).toBe(45)
    expect(job.settings.grayscale).toBe(true)
    expect(job.settings.outputDir).toBe('/tmp/custom-out')
    expect(job.useRecommendedSettings).toBe(false)
  })

  it('still applies recommended settings to newly added files', async () => {
    mockedAnalyze.mockResolvedValue(analysisResponse({ recommendedPreset: 'maximum' }))

    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/fresh.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value[0]?.status).toBe('ready')
    })

    expect(composable.jobs.value[0].settings.preset).toBe('maximum')
    expect(composable.jobs.value[0].useRecommendedSettings).toBe(true)
  })
})
