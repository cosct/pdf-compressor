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

export function formatMilliseconds(value?: number): string {
  if (value === undefined || !Number.isFinite(value)) {
    return '--'
  }

  if (value < 1000) {
    return `${Math.round(value)} ms`
  }

  return `${(value / 1000).toFixed(1)} s`
}

export function fileNameFromPath(path: string): string {
  const segments = path.split(/[/\\]/)
  return segments[segments.length - 1] || path
}

export function directoryFromPath(path: string): string {
  const segments = path.split(/[/\\]/)
  if (segments.length <= 1) {
    return path
  }

  return segments.slice(0, -1).join(' / ')
}
