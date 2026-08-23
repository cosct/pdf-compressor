/**
 * Tauri native bridge — typed layer between Vue and the Rust backend.
 * Tauri 原生桥接层 — Vue 与 Rust 后端之间的类型化封装。
 *
 * Command types and call wrappers are generated from the Rust source by
 * tauri-specta (`src/lib/bindings.ts`, regenerated via
 * `cd src-tauri && cargo test export_bindings`).
 * 命令类型与调用封装由 tauri-specta 从 Rust 源码生成。
 */
import { Channel, invoke } from '@tauri-apps/api/core'

import type {
  AnalysisResponse as AnalysisResponseWire,
  CompressionResponse as CompressionResponseWire,
  PresetUserConfigPayload,
  ProgressUpdate as ProgressUpdateWire,
} from './bindings'
import { commands } from './bindings'

import type { CompressionSettings, PresetUserConfig, ProgressUpdate } from '../types/pdf'
import { normalizeMessage } from '../composables/backendMessages'
import { calculateMaxImageSizePx } from '../utils/compressionSettings'

export type { AnalysisResponse, CompressionResponse } from './bindings'

export function hasNativeCommands(): boolean {
  return typeof invoke === 'function'
}

/** Unwrap a tauri-specta Result-style payload, rethrowing the error payload. */
async function unwrap<T>(result: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: unknown }>): Promise<T> {
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
    message: message?.message ? normalizeMessage(message.message) : null,
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
  onProgress?: (update: ProgressUpdate) => void,
): Promise<AnalysisResponseWire> {
  const channel = createProgressChannel(onProgress)
  return unwrap(commands.analyzePdf(path, path, channel))
}

export async function compressPdf(
  path: string,
  settings: CompressionSettings,
  taskId: string,
  onProgress?: (update: ProgressUpdate) => void,
): Promise<CompressionResponseWire> {
  const maxImageSizePx = calculateMaxImageSizePx(
    settings.maxImageSizePercent,
    settings.referenceMaxImageEdgePx,
  )

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
        settings: {
          preset: settings.preset,
          imageQuality: settings.imageQuality,
          maxImageSizePx,
          optimizeImages: settings.optimizeImages,
          compressStreams: settings.compressStreams,
          stripMetadata: settings.stripMetadata,
          outputDir: settings.outputDir,
        },
        preset: null,
        imageQuality: null,
        maxImageSizePx: null,
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

export async function cancelCompression(taskId: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await unwrap(commands.cancelCompression(taskId))
}

// ---------------------------------------------------------------------------
// Preset config persistence
// ---------------------------------------------------------------------------

function toPresetUserConfig(payload: PresetUserConfigPayload): PresetUserConfig {
  return {
    version: payload.version ?? 1,
    presets: { ...payload.presets },
  }
}

export async function loadPresetUserConfig(): Promise<PresetUserConfig> {
  if (!hasNativeCommands()) {
    return {
      version: 1,
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
export async function listenForOpenPdf(
  listener: (paths: string[]) => void,
): Promise<() => void> {
  if (!hasNativeCommands()) {
    return () => {}
  }

  const { listen } = await import('@tauri-apps/api/event')
  return listen<{ paths: string[] }>('open-pdf', (event) => {
    const paths = Array.isArray(event.payload?.paths)
      ? event.payload.paths.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
      : []
    if (paths.length) {
      listener(paths)
    }
  })
}
