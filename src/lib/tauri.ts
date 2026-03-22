import type { CompressionSettings } from '../types/pdf'

type InvokeArgs = Record<string, unknown> | undefined

export function hasNativeCommands(): boolean {
  return typeof window.__TAURI_INTERNALS__?.invoke === 'function'
}

export type NativePdfDropEvent =
  | { type: 'enter'; paths: string[] }
  | { type: 'over' }
  | { type: 'drop'; paths: string[] }
  | { type: 'leave' }

export async function invokeCommand<T>(command: string, args?: InvokeArgs): Promise<T> {
  const tauriInternals = window.__TAURI_INTERNALS__

  if (!tauriInternals?.invoke) {
    throw new Error('Native commands are available only inside the desktop app shell.')
  }

  return tauriInternals.invoke<T>(command, args)
}

export async function openPdfDialog(): Promise<string | null> {
  const selection = await invokeCommand<string | string[] | null>('plugin:dialog|open', {
    options: {
      multiple: false,
      directory: false,
      filters: [{ name: 'PDF documents', extensions: ['pdf'] }],
    },
  })

  if (Array.isArray(selection)) {
    return selection[0] ?? null
  }

  return selection
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

export async function analyzePdf(path: string): Promise<unknown> {
  return invokeCommand<unknown>('analyze_pdf', {
    path,
    inputPath: path,
  })
}

export async function compressPdf(path: string, settings: CompressionSettings): Promise<unknown> {
  return invokeCommand<unknown>('compress_pdf', {
    path,
    inputPath: path,
    settings,
    preset: settings.preset,
    imageQuality: settings.imageQuality,
    maxImageSizePx: settings.maxImageSizePx,
    optimizeImages: settings.optimizeImages,
    compressStreams: settings.compressStreams,
    stripMetadata: settings.stripMetadata,
    removeMetadata: settings.stripMetadata,
  })
}
