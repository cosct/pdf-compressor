/**
 * Display formatting helpers for the UI layer.
 * UI 层的显示格式化工具函数。
 */

export function formatBytes(bytes?: number): string {
  if (bytes === undefined || !Number.isFinite(bytes)) {
    return '--'
  }

  if (bytes < 1024) {
    return `${bytes} B`
  }

  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1024
  let unitIndex = 0

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }

  const precision = value >= 100 ? 0 : value >= 10 ? 1 : 2

  return `${value.toFixed(precision)} ${units[unitIndex]}`
}

export function formatPercent(value?: number | null): string {
  if (value === undefined || value === null || !Number.isFinite(value)) {
    return '--'
  }

  return `${value.toFixed(value >= 10 ? 0 : 1)}%`
}

export function fileNameFromPath(path: string): string {
  const segments = path.split(/[/\\]/)
  return segments[segments.length - 1] || path
}

export function isPdfPath(path: string): boolean {
  return path.trim().toLowerCase().endsWith('.pdf')
}

/** Compact duration for result reports: `830 ms`, `4.2 s`, `2 min 05 s`. */
export function formatDuration(ms?: number): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms) || ms < 0) {
    return '--'
  }

  if (ms < 1_000) {
    return `${Math.round(ms)} ms`
  }

  const totalSeconds = ms / 1_000
  if (totalSeconds < 60) {
    return `${totalSeconds.toFixed(1)} s`
  }

  const minutes = Math.floor(totalSeconds / 60)
  const seconds = Math.round(totalSeconds % 60)
  return `${minutes} min ${String(seconds).padStart(2, '0')} s`
}
