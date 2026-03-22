export {}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>
    }
  }
}
