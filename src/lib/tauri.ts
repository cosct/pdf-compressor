/**
 * Tauri native bridge — thin layer between Vue and Rust backend commands.
 * Tauri 原生桥接层 — Vue 与 Rust 后端命令之间的薄封装。
 *
 * Provides file/directory dialogs, native drag-and-drop listening,
 * PDF analysis/compression invocation, compression cancellation,
 * preset config persistence, and window management (minimize, maximize, close, drag).
 * 提供文件/目录对话框、原生拖放监听、PDF 分析/压缩调用、压缩取消、
 * 预设配置持久化和窗口管理（最小化、最大化、关闭、拖动）。
 */
import { Channel, invoke } from '@tauri-apps/api/core'

import type { CompressionSettings, PresetUserConfig, ProgressUpdate } from '../types/pdf'
import { calculateMaxImageSizePx } from '../utils/compressionSettings'

type InvokeArgs = Record<string, unknown> | undefined

export function hasNativeCommands(): boolean {
  return typeof invoke === 'function'
}

export type NativePdfDropEvent =
  | { type: 'enter'; paths: string[] }
  | { type: 'over' }
  | { type: 'drop'; paths: string[] }
  | { type: 'leave' }

export async function invokeCommand<T>(command: string, args?: InvokeArgs): Promise<T> {
  if (!hasNativeCommands()) {
    throw new Error('Native commands are available only inside the desktop app shell.')
  }

  return invoke<T>(command, args)
}

export async function openPdfDialog(multiple = false): Promise<string[]> {
  const selection = await invokeCommand<string | string[] | null>('plugin:dialog|open', {
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
  const selection = await invokeCommand<string | string[] | null>('plugin:dialog|open', {
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

function createProgressChannel(onProgress?: (update: ProgressUpdate) => void) {
  const channel = new Channel<ProgressUpdate>()
  channel.onmessage = (message) => {
    onProgress?.(message)
  }
  return channel
}

export async function analyzePdf(
  path: string,
  onProgress?: (update: ProgressUpdate) => void,
): Promise<unknown> {
  return invokeCommand<unknown>('analyze_pdf', {
    path,
    inputPath: path,
    onProgress: createProgressChannel(onProgress),
  })
}

export async function compressPdf(
  path: string,
  settings: CompressionSettings,
  taskId: string,
  onProgress?: (update: ProgressUpdate) => void,
): Promise<unknown> {
  const maxImageSizePx = calculateMaxImageSizePx(
    settings.maxImageSizePercent,
    settings.referenceMaxImageEdgePx,
  )

  return invokeCommand<unknown>('compress_pdf', {
    request: {
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
    },
    onProgress: createProgressChannel(onProgress),
  })
}

export async function cancelCompression(taskId: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await invokeCommand<void>('cancel_compression', { taskId })
}

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

export async function notifyAppReady(): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await invokeCommand<void>('app_ready')
}

export async function loadPresetUserConfig(): Promise<PresetUserConfig> {
  if (!hasNativeCommands()) {
    return {
      version: 1,
      presets: {},
    }
  }

  return invokeCommand<PresetUserConfig>('load_preset_user_config')
}

export async function savePresetUserConfig(config: PresetUserConfig): Promise<PresetUserConfig> {
  if (!hasNativeCommands()) {
    return config
  }

  return invokeCommand<PresetUserConfig>('save_preset_user_config', { config })
}

export async function clearPresetUserConfig(): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await invokeCommand<void>('clear_preset_user_config')
}

export async function openPath(path: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await invokeCommand<void>('open_path', { path })
}

export async function revealPathInFolder(path: string): Promise<void> {
  if (!hasNativeCommands()) {
    return
  }

  await invokeCommand<void>('reveal_path_in_folder', { path })
}
