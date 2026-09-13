/**
 * Tauri native bridge — typed layer between Vue and the Rust backend.
 * Tauri 原生桥接层 — Vue 与 Rust 后端之间的类型化封装。
 *
 * Command types and call wrappers are generated from the Rust source by
 * tauri-specta (`src/lib/bindings.ts`, regenerated via
 * `cd src-tauri && cargo test export_bindings`).
 * 命令类型与调用封装由 tauri-specta 从 Rust 源码生成。
 */
import { Channel, invoke, isTauri } from '@tauri-apps/api/core'

import type {
  AnalysisResponse as AnalysisResponseWire,
  CompressionResponse as CompressionResponseWire,
  PresetUserConfigPayload,
  QuickProfilePayload,
  ProgressUpdate as ProgressUpdateWire,
} from './bindings'
import { commands } from './bindings'

import type { CompressionSettings, PresetUserConfig, ProgressUpdate } from '../types/pdf'

export type { AnalysisResponse, CompressionResponse } from './bindings'

/**
 * True only inside a Tauri webview. `invoke` is a plain bundled function, so
 * checking its type would report native availability in a plain browser too.
 */
export function hasNativeCommands(): boolean {
  return isTauri()
}

/** Unwrap a tauri-specta Result-style payload, rethrowing the error payload. */
async function unwrap<T>(
  result: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: unknown }>,
): Promise<T> {
  const outcome = await result
  if (outcome.status === 'error') {
    throw outcome.error
  }
  return outcome.data
}

const PROGRESS_PHASES = ['queued', 'analyzing', 'compressing', 'writing', 'done', 'error'] as const

function normalizeProgressUpdate(message: ProgressUpdateWire | undefined): ProgressUpdate {
  const phase = PROGRESS_PHASES.includes(message?.phase as (typeof PROGRESS_PHASES)[number])
    ? (message!.phase as (typeof PROGRESS_PHASES)[number])
    : 'compressing'
  const percent = Number(message?.percent)
  return {
    phase,
    percent: Number.isFinite(percent) ? percent : 0,
  }
}

export type NativePdfDropEvent =
  | { type: 'enter'; paths: string[] }
  | { type: 'over' }
  | { type: 'drop'; paths: string[] }
  | { type: 'leave' }

function createProgressChannel(onProgress?: (update: ProgressUpdate) => void) {
  const channel = new Channel<ProgressUpdateWire>()
  channel.onmessage = (message) => {
    onProgress?.(normalizeProgressUpdate(message))
  }
  return channel
}

// ---------------------------------------------------------------------------
// PDF analysis / compression
// ---------------------------------------------------------------------------

export async function analyzePdf(
  path: string,
  password: string | null | undefined,
  onProgress?: (update: ProgressUpdate) => void,
  settings?: CompressionSettings | null,
): Promise<AnalysisResponseWire> {
  const channel = createProgressChannel(onProgress)
  // The optional settings context makes the estimate follow the job's live
  // toggles (CMYK conversion stance, size cap, preset) instead of the
  // default posture. Only percent travels: the engine resolves it against
  // the document's own largest image edge.
  const settingsPayload = settings
    ? {
        preset: settings.preset,
        imageQuality: settings.imageQuality,
        maxImageSizePercent: settings.maxImageSizePercent,
        optimizeImages: settings.optimizeImages,
        compressStreams: settings.compressStreams,
        stripMetadata: settings.stripMetadata,
        grayscale: settings.grayscale,
        bilevelCodec: settings.bilevelCodec,
        subsetFonts: settings.subsetFonts,
        cmykConversion: settings.cmykConversion,
        outputDir: null,
      }
    : null
  return unwrap(commands.analyzePdf(path, path, password ?? null, settingsPayload, channel))
}

export async function compressPdf(
  path: string,
  settings: CompressionSettings,
  taskId: string,
  onProgress?: (update: ProgressUpdate) => void,
  password?: string | null,
): Promise<CompressionResponseWire> {
  const channel = createProgressChannel(onProgress)
  const targetSizeBytes = settings.targetFileSizeMb
    ? Math.max(1, Math.round(settings.targetFileSizeMb * 1024 * 1024))
    : null

  return unwrap(
    commands.compressPdf(
      {
        path,
        inputPath: path,
        taskId,
        password: password?.trim() ? password.trim() : null,
        settings: {
          preset: settings.preset,
          imageQuality: settings.imageQuality,
          // 0.9.0 unified semantics: the cap travels as a percent of the
          // document's largest image edge and the engine resolves it after
          // loading — the same adaptive numbers the GUI previews.
          maxImageSizePx: null,
          maxImageSizePercent: settings.maxImageSizePercent,
          optimizeImages: settings.optimizeImages,
          compressStreams: settings.compressStreams,
          stripMetadata: settings.stripMetadata,
          grayscale: settings.grayscale,
          bilevelCodec: settings.bilevelCodec,
          subsetFonts: settings.subsetFonts,
          cmykConversion: settings.cmykConversion,
          outputDir: settings.outputDir,
        },
        preset: null,
        imageQuality: null,
        maxImageSizePx: null,
        maxImageSizePercent: null,
        optimizeImages: null,
        compressStreams: null,
        stripMetadata: null,
        removeMetadata: null,
        outputDir: null,
        targetSizeBytes,
      },
      channel,
    ),
  )
}

/**
 * Scanned-document pipeline — same engine, but the backend forces image and
 * stream optimization on (they are where scan savings live) and accepts the
 * grayscale switch as a first-class override.
 */
export async function compressScannedPdf(
  path: string,
  settings: CompressionSettings,
  taskId: string,
  onProgress?: (update: ProgressUpdate) => void,
  password?: string | null,
): Promise<CompressionResponseWire> {
  const channel = createProgressChannel(onProgress)
  const targetSizeBytes = settings.targetFileSizeMb
    ? Math.max(1, Math.round(settings.targetFileSizeMb * 1024 * 1024))
    : null

  return unwrap(
    commands.compressScannedPdf(
      {
        path,
        inputPath: path,
        taskId,
        password: password?.trim() ? password.trim() : null,
        settings: {
          preset: settings.preset,
          imageQuality: settings.imageQuality,
          maxImageSizePx: null,
          maxImageSizePercent: settings.maxImageSizePercent,
          optimizeImages: settings.optimizeImages,
          compressStreams: settings.compressStreams,
          stripMetadata: settings.stripMetadata,
          grayscale: settings.grayscale,
          bilevelCodec: settings.bilevelCodec,
          subsetFonts: settings.subsetFonts,
          cmykConversion: settings.cmykConversion,
          outputDir: settings.outputDir,
        },
        preset: null,
        imageQuality: null,
        maxImageSizePx: null,
        maxImageSizePercent: null,
        grayscale: settings.grayscale,
        bilevelCodec: settings.bilevelCodec,
        stripMetadata: null,
        removeMetadata: null,
        outputDir: null,
        targetSizeBytes,
      },
      channel,
    ),
  )
}

export async function cancelCompression(taskId: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.cancelCompression(taskId))
}

/**
 * Filter paths down to those still present on disk — used to pre-check a
 * restored session queue so missing files are skipped up front.
 */
export async function existingPaths(paths: string[]): Promise<string[]> {
  if (!hasNativeCommands()) {
    return paths
  }

  return unwrap(commands.existingPaths(paths))
}

// ---------------------------------------------------------------------------
// Build capabilities (0.9.0 honesty pass)
// ---------------------------------------------------------------------------

/** Engine components compiled into this build; `null` in browser preview. */
export type EngineBuildFeatures = {
  cmykCms: boolean
  jpx: boolean
  ccitt: boolean
  subsetFonts: boolean
}

let cachedBuildFeatures: EngineBuildFeatures | null = null

export async function getBuildFeatures(): Promise<EngineBuildFeatures | null> {
  if (!hasNativeCommands()) {
    return null
  }
  cachedBuildFeatures ??= await unwrap(commands.buildFeatures())
  return cachedBuildFeatures
}

// ---------------------------------------------------------------------------
// Preset config persistence
// ---------------------------------------------------------------------------

function toPresetUserConfig(payload: PresetUserConfigPayload): PresetUserConfig {
  return {
    version: payload.version ?? 2,
    presets: { ...payload.presets },
  }
}

export async function loadPresetUserConfig(): Promise<PresetUserConfig> {
  if (!hasNativeCommands()) {
    return {
      version: 2,
      presets: {},
    }
  }

  return toPresetUserConfig(await unwrap(commands.loadPresetUserConfig()))
}

export async function savePresetUserConfig(config: PresetUserConfig): Promise<PresetUserConfig> {
  if (!hasNativeCommands()) {
    return config
  }

  return toPresetUserConfig(
    await unwrap(
      commands.savePresetUserConfig({
        version: config.version,
        presets: { ...config.presets },
      }),
    ),
  )
}

export async function clearPresetUserConfig(): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.clearPresetUserConfig())
}

// ---------------------------------------------------------------------------
// Quick-mode (right-click) profile — persisted by the backend at the shared
// OS-config location so `pdf-compressor-cli quick` picks up these settings.
// Browser preview keeps a session-local copy only.
// ---------------------------------------------------------------------------

let browserQuickProfile: QuickProfilePayload | null = null

export async function getQuickProfile(): Promise<QuickProfilePayload> {
  if (!hasNativeCommands()) {
    return browserQuickProfile ?? { version: 2 }
  }

  return unwrap(commands.loadQuickProfile())
}

export async function saveQuickProfile(profile: QuickProfilePayload): Promise<QuickProfilePayload> {
  if (!hasNativeCommands()) {
    browserQuickProfile = { ...profile }
    return profile
  }

  return unwrap(commands.saveQuickProfile(profile))
}

// ---------------------------------------------------------------------------
// App update checks (plugin:updater + plugin:process)
// ---------------------------------------------------------------------------

/** Version metadata for an available update. */
export type AppUpdateInfo = {
  version: string
  currentVersion: string
  notes: string | null
}

/** Result of an update check: either an available update or the current version. */
export type AppUpdateCheck =
  | { available: true; info: AppUpdateInfo }
  | { available: false; currentVersion: string }

let pendingUpdate: import('@tauri-apps/plugin-updater').Update | null = null

export async function checkForAppUpdate(): Promise<AppUpdateCheck> {
  if (!hasNativeCommands()) {
    throw new Error('update checks require the desktop app')
  }
  const [{ check }, { getVersion }] = await Promise.all([
    import('@tauri-apps/plugin-updater'),
    import('@tauri-apps/api/app'),
  ])
  const [update, currentVersion] = await Promise.all([check(), getVersion()])
  pendingUpdate = update
  return update
    ? {
        available: true,
        info: {
          version: update.version,
          currentVersion,
          notes: update.body ?? null,
        },
      }
    : { available: false, currentVersion }
}

export async function downloadAndInstallAppUpdate(
  onProgress?: (downloadedBytes: number, totalBytes: number | undefined) => void,
): Promise<void> {
  const update = pendingUpdate
  if (!update) {
    throw new Error('no update pending — run a check first')
  }

  let total: number | undefined
  let downloaded = 0
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case 'Started':
        total = event.data.contentLength
        onProgress?.(0, total)
        break
      case 'Progress':
        downloaded += event.data.chunkLength
        onProgress?.(downloaded, total)
        break
      case 'Finished':
        onProgress?.(total ?? downloaded, total)
        break
    }
  })
  pendingUpdate = null
}

export async function relaunchApp(): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

export async function openPath(path: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.openPath(path))
}

export async function revealPathInFolder(path: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.revealPathInFolder(path))
}

export async function notifyAppReady(): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.appReady())
}

// ---------------------------------------------------------------------------
// Dialogs (plugin:dialog)
// ---------------------------------------------------------------------------

export async function openPdfDialog(multiple = false): Promise<string[]> {
  const selection = await invoke<string | string[] | null>('plugin:dialog|open', {
    options: {
      multiple,
      directory: false,
      filters: [{ name: 'PDF documents', extensions: ['pdf'] }],
    },
  })

  if (Array.isArray(selection)) {
    return selection.filter(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    )
  }

  return typeof selection === 'string' && selection.trim().length > 0 ? [selection] : []
}

export async function openDirectoryDialog(): Promise<string | null> {
  const selection = await invoke<string | string[] | null>('plugin:dialog|open', {
    options: {
      multiple: false,
      directory: true,
    },
  })

  if (Array.isArray(selection)) {
    const first = selection.find(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    )
    return first ?? null
  }

  return typeof selection === 'string' && selection.trim().length > 0 ? selection : null
}

// ---------------------------------------------------------------------------
// Native drag-and-drop
// ---------------------------------------------------------------------------

export async function listenForNativePdfDrop(
  listener: (event: NativePdfDropEvent) => void,
): Promise<() => void> {
  if (!hasNativeCommands()) {
    return () => {}
  }

  const { getCurrentWebview } = await import('@tauri-apps/api/webview')

  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload

    switch (payload.type) {
      case 'enter':
        listener({ type: 'enter', paths: payload.paths })
        break
      case 'over':
        listener({ type: 'over' })
        break
      case 'drop':
        listener({ type: 'drop', paths: payload.paths })
        break
      case 'leave':
        listener({ type: 'leave' })
        break
    }
  })
}

// ---------------------------------------------------------------------------
// Window management
// ---------------------------------------------------------------------------

async function getCurrentAppWindow() {
  if (!hasNativeCommands()) {
    return null
  }

  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  return getCurrentWindow()
}

export async function minimizeAppWindow(): Promise<void> {
  const currentWindow = await getCurrentAppWindow()
  await currentWindow?.minimize()
}

export async function toggleAppWindowMaximize(): Promise<boolean> {
  const currentWindow = await getCurrentAppWindow()

  if (!currentWindow) {
    return false
  }

  const maximized = await currentWindow.isMaximized()

  if (maximized) {
    await currentWindow.unmaximize()
    return false
  }

  await currentWindow.maximize()
  return true
}

export async function isAppWindowMaximized(): Promise<boolean> {
  const currentWindow = await getCurrentAppWindow()
  return currentWindow ? currentWindow.isMaximized() : false
}

export async function closeAppWindow(): Promise<void> {
  const currentWindow = await getCurrentAppWindow()
  await currentWindow?.close()
}

export async function startDraggingAppWindow(): Promise<void> {
  const currentWindow = await getCurrentAppWindow()
  await currentWindow?.startDragging()
}

/**
 * Listen for PDFs handed to an already-running instance via "Open with…"
 * (single-instance plugin forwards argv paths as the `open-pdf` event).
 */
export async function listenForOpenPdf(listener: (paths: string[]) => void): Promise<() => void> {
  if (!hasNativeCommands()) {
    return () => {}
  }

  const { listen } = await import('@tauri-apps/api/event')
  return listen<{ paths: string[] }>('open-pdf', (event) => {
    const paths = Array.isArray(event.payload?.paths)
      ? event.payload.paths.filter(
          (item): item is string => typeof item === 'string' && item.trim().length > 0,
        )
      : []
    if (paths.length) {
      listener(paths)
    }
  })
}
