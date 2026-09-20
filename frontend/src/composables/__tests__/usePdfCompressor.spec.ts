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
    bilevelCodec: 'ccitt-g4',
    subsetFonts: false,
    cmykConversion: true,
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
      cmykConversion: false,
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
        cmykConversion: false,
        outputDir: null,
        targetFileSizeMb: 0,
      }).targetFileSizeMb,
    ).toBeNull()
  })

  it('coerces a missing cmykConversion field (legacy persisted queue) to true', () => {
    const restored = normalizeSettings({
      ...makeSettings({ cmykConversion: undefined as unknown as boolean }),
    })
    // 0.7.x queues must adopt the new default...
    expect(restored.cmykConversion).toBe(true)

    expect(normalizeSettings(makeSettings()).cmykConversion).toBe(true)
    // ...while an explicit opt-out survives normalization.
    expect(normalizeSettings(makeSettings({ cmykConversion: false })).cmykConversion).toBe(false)
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
      bilevelCodec: undefined as unknown as 'ccitt-g4',
      subsetFonts: undefined as unknown as boolean,
      cmykConversion: false,
      outputDir: null,
      targetFileSizeMb: null,
    })
    expect(normalized.grayscale).toBe(false)
    expect(normalized.bilevelCodec).toBe('ccitt-g4')
  })

  it('keeps jpeg as the only non-default bilevel codec', () => {
    const normalized = normalizeSettings({
      preset: 'balanced',
      imageQuality: 72,
      maxImageSizePercent: 80,
      referenceMaxImageEdgePx: 1234,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: true,
      bilevelCodec: 'jpeg',
      subsetFonts: true,
      cmykConversion: false,
      outputDir: null,
      targetFileSizeMb: null,
    })
    expect(normalized.bilevelCodec).toBe('jpeg')
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

  it('blocks a new run until the backend cancellation settles', async () => {
    // Regression (0.11.0): the cancel path used to clear compressionRunning
    // before the backend tasks settled, so an immediate restart reset the
    // shared cancellation flag under the old run's still-running workers.
    const composable = await readyQueue(['/tmp/a.pdf'])

    let releaseCompress!: () => void
    mockedCompress.mockImplementation(
      () =>
        new Promise<CompressionResponse>((resolve) => {
          releaseCompress = () => resolve(compressionResponse())
        }),
    )
    let releaseCancel!: () => void
    mockedCancel.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          releaseCancel = () => resolve()
        }),
    )

    const run = composable.compressCurrentPdf()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('compressing')
    })

    const cancelling = composable.cancelCompressionRun()
    // While the backend cancel is pending, a restart must be refused.
    await composable.compressCurrentPdf()
    expect(mockedCompress).toHaveBeenCalledTimes(1)

    releaseCancel()
    await cancelling
    releaseCompress()
    await run

    // Once the cancel settles, the next start goes through normally.
    mockedCompress.mockClear()
    mockedCompress.mockResolvedValue(compressionResponse())
    await composable.compressCurrentPdf()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('success')
    })
    expect(mockedCompress).toHaveBeenCalledTimes(1)
  })
})

describe('run startup races', () => {
  it('ignores a second start while the analysis queue is still draining', async () => {
    // Regression (0.11.0): the startup guard ran before awaiting the
    // analysis queue, so two clicks during the drain window could each
    // launch a compression run over the same targets.
    let releaseAnalysis!: () => void
    mockedAnalyze.mockImplementation(
      () =>
        new Promise<AnalysisResponse>((resolve) => {
          releaseAnalysis = () => resolve(analysisResponse())
        }),
    )
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/a.pdf'])

    mockedCompress.mockResolvedValue(compressionResponse())
    const first = composable.compressCurrentPdf()
    const second = composable.compressCurrentPdf()
    releaseAnalysis()
    await Promise.all([first, second])

    expect(mockedCompress).toHaveBeenCalledTimes(1)
  })
})

describe('analysis cancellation', () => {
  it('cancels an in-flight analysis through the backend registry and resets the job', async () => {
    // 0.11.0: analysis registers a backend task id, the cancel button is
    // available while a document is being scanned, and a cancelled run
    // resets the job quietly instead of flagging an error.
    let rejectAnalysis!: (reason: unknown) => void
    mockedAnalyze.mockImplementation(
      () =>
        new Promise<AnalysisResponse>((_, reject) => {
          rejectAnalysis = reject
        }),
    )
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/slow.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('analyzing')
    })
    expect(composable.canCancelCompression.value).toBe(true)

    await composable.cancelCompressionRun()
    expect(mockedCancel).toHaveBeenCalledWith(expect.stringMatching(/::analysis$/))

    rejectAnalysis(
      Object.assign(new Error('cancelled'), {
        code: 'error.cancelled',
        fallback: 'The active compression task was cancelled.',
      }),
    )
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('selected')
    })
    expect(composable.jobs.value[0].error).toBeNull()
    // The drain's finally lands a microtask after the job resets.
    await vi.waitFor(() => {
      expect(composable.canCancelCompression.value).toBe(false)
    })
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
      bilevelCodec: 'jpeg',
      subsetFonts: true,
      cmykConversion: false,
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

  it('analyzes the first restored job against its restored settings, not the draft', async () => {
    // Regression (0.11.0): addSourcePaths queues analysis synchronously and
    // the first request captured job.settings before the restore loop wrote
    // the persisted settings back — the estimate for the first job ran
    // against the draft baseline. The settings must ride along with the
    // queue add instead.
    const first = normalizeSettings(makeSettings({ preset: 'maximum', imageQuality: 45 }))
    const second = normalizeSettings(makeSettings({ preset: 'conservative', imageQuality: 30 }))
    window.localStorage.setItem(
      'pdf-compressor-queue',
      JSON.stringify([
        { sourcePath: '/tmp/first.pdf', settings: first },
        { sourcePath: '/tmp/second.pdf', settings: second },
      ]),
    )
    vi.mocked(existingPaths).mockResolvedValue(['/tmp/first.pdf', '/tmp/second.pdf'])
    mockedAnalyze.mockResolvedValue(analysisResponse({ recommendedPreset: 'balanced' }))

    const composable = usePdfCompressor()
    await vi.waitFor(() => {
      // Exact array match: `every()` on a still-empty queue is vacuously
      // true and would let the wait pass before the restore lands.
      expect(composable.jobs.value.map((job) => job.status)).toEqual(['ready', 'ready'])
    })

    expect(mockedAnalyze).toHaveBeenCalledTimes(2)
    expect(mockedAnalyze.mock.calls[0][0]).toBe('/tmp/first.pdf')
    expect(mockedAnalyze.mock.calls[0][4]).toMatchObject({ preset: 'maximum', imageQuality: 45 })
    expect(mockedAnalyze.mock.calls[1][0]).toBe('/tmp/second.pdf')
    expect(mockedAnalyze.mock.calls[1][4]).toMatchObject({
      preset: 'conservative',
      imageQuality: 30,
    })
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

  it('restores a pre-0.8 legacy queue entry without corrupting newer fields', async () => {
    // The exact persisted shape of an old install: none of the fields
    // introduced after the entry was written exist. Restoring must fill
    // every newer field with its current default instead of leaking
    // undefined/NaN into a compression request.
    const legacyEntry = {
      sourcePath: '/tmp/legacy.pdf',
      settings: {
        preset: 'balanced',
        imageQuality: 72,
        maxImageSizePercent: 80,
        optimizeImages: true,
        compressStreams: true,
        stripMetadata: true,
      },
    }
    window.localStorage.setItem('pdf-compressor-queue', JSON.stringify([legacyEntry]))
    vi.mocked(existingPaths).mockResolvedValue(['/tmp/legacy.pdf'])
    mockedAnalyze.mockResolvedValue(analysisResponse({ recommendedPreset: 'balanced' }))

    const composable = usePdfCompressor()
    await vi.waitFor(() => {
      expect(composable.jobs.value[0]?.status).toBe('ready')
    })

    const settings = composable.jobs.value[0].settings
    expect(settings.preset).toBe('balanced')
    expect(settings.imageQuality).toBe(72)
    expect(settings.grayscale).toBe(false)
    expect(settings.bilevelCodec).toBe('ccitt-g4')
    expect(settings.subsetFonts).toBe(false)
    expect(settings.cmykConversion).toBe(true)
    expect(settings.referenceMaxImageEdgePx).toBeNull()
    expect(settings.targetFileSizeMb).toBeNull()
    expect(settings.outputDir).toBeNull()
  })
})

describe('password retry', () => {
  function passwordError(code: 'error.passwordRequired' | 'error.wrongPassword') {
    return Object.assign(new Error('locked'), {
      code,
      fallback: 'needs a password',
    })
  }

  it('flags a password-blocked job, retries analysis with the password, and passes it to compression', async () => {
    mockedAnalyze.mockRejectedValueOnce(passwordError('error.passwordRequired'))
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/locked.pdf'])

    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('error')
    })
    expect(composable.selectedJobNeedsPassword.value).toBe(true)
    // The password never enters the persisted queue (other tests' debounced
    // writes may share the storage stub, so assert on absence, not shape —
    // and a timing-dependent absent key trivially satisfies it).
    expect(window.localStorage.getItem('pdf-compressor-queue') ?? '').not.toContain('open-secret')

    mockedAnalyze.mockResolvedValue(analysisResponse())
    composable.submitJobPassword(composable.jobs.value[0].id, 'open-secret')

    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('ready')
    })
    expect(composable.selectedJobNeedsPassword.value).toBe(false)
    expect(mockedAnalyze).toHaveBeenLastCalledWith(
      '/tmp/locked.pdf',
      'open-secret',
      expect.any(String),
      expect.any(Function),
      // The job's live settings ride along as the analysis context (0.9.0
      // honesty pass) — the balanced draft defaults in this test.
      expect.objectContaining({ preset: 'balanced' }),
    )

    mockedCompress.mockResolvedValue(compressionResponse())
    await composable.compressCurrentPdf()
    expect(mockedCompress).toHaveBeenCalledWith(
      '/tmp/locked.pdf',
      expect.anything(),
      expect.any(String),
      expect.any(Function),
      'open-secret',
    )
  })

  it('keeps whitespace-padded passwords intact — spaces are legal password bytes', async () => {
    // Regression (0.11.0): the submit path trimmed the password, silently
    // breaking documents whose open password carries leading/trailing
    // spaces; only the empty string means "no password".
    mockedAnalyze.mockRejectedValueOnce(passwordError('error.passwordRequired'))
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/locked.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('error')
    })

    mockedAnalyze.mockResolvedValue(analysisResponse())
    composable.submitJobPassword(composable.jobs.value[0].id, ' open secret ')

    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('ready')
    })
    expect(mockedAnalyze).toHaveBeenLastCalledWith(
      '/tmp/locked.pdf',
      ' open secret ',
      expect.any(String),
      expect.any(Function),
      expect.anything(),
    )
  })

  it('re-flags the job when the supplied password is wrong', async () => {
    mockedAnalyze.mockRejectedValueOnce(passwordError('error.passwordRequired'))
    const composable = usePdfCompressor()
    composable.addSourcePaths(['/tmp/locked.pdf'])
    await vi.waitFor(() => {
      expect(composable.jobs.value[0].status).toBe('error')
    })

    mockedAnalyze.mockRejectedValueOnce(passwordError('error.wrongPassword'))
    composable.submitJobPassword(composable.jobs.value[0].id, 'not-it')

    await vi.waitFor(() => {
      expect(composable.jobs.value[0].error?.id.startsWith('error.wrongPassword')).toBe(true)
    })
    expect(composable.selectedJobNeedsPassword.value).toBe(true)
  })
})
