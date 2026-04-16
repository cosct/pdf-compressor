/**
 * Compression settings clamping and normalization utilities.
 * 压缩设置的夹紧与规范化工具函数。
 *
 * Provides range-safe clamping for image quality and max image size,
 * and converts a percentage-based max image size to absolute pixel values
 * using a reference edge (typically from analysis).
 * 提供图片质量和最大图片尺寸的安全范围夹紧，
 * 并将百分比式最大图片尺寸转换为绝对像素值（参考边长通常来自分析结果）。
 */

export const MIN_IMAGE_QUALITY = 1
export const MAX_IMAGE_QUALITY = 100
export const MIN_IMAGE_SIZE_PERCENT = 5
export const MAX_IMAGE_SIZE_PERCENT = 100
export const MIN_IMAGE_SIZE_PX = 100
export const MAX_IMAGE_SIZE_PX = 20000
export const DEFAULT_REFERENCE_IMAGE_EDGE_PX = 3200

export function clampImageQuality(value: number): number {
  return Math.max(MIN_IMAGE_QUALITY, Math.min(MAX_IMAGE_QUALITY, Math.round(value)))
}

export function clampMaxImageSizePercent(value: number): number {
  return Math.max(MIN_IMAGE_SIZE_PERCENT, Math.min(MAX_IMAGE_SIZE_PERCENT, Math.round(value)))
}

export function clampMaxImageSizePx(value: number): number {
  return Math.max(MIN_IMAGE_SIZE_PX, Math.min(MAX_IMAGE_SIZE_PX, Math.round(value / 100) * 100))
}

export function normalizeReferenceMaxImageEdgePx(value?: number | null): number | null {
  if (!Number.isFinite(value) || (value ?? 0) <= 0) {
    return null
  }

  return Math.round(value as number)
}

export function calculateMaxImageSizePx(
  maxImageSizePercent: number,
  referenceMaxImageEdgePx?: number | null,
): number {
  const baseEdge = normalizeReferenceMaxImageEdgePx(referenceMaxImageEdgePx) ?? DEFAULT_REFERENCE_IMAGE_EDGE_PX
  const calculated = Math.round((baseEdge * clampMaxImageSizePercent(maxImageSizePercent)) / 100 / 100) * 100
  return clampMaxImageSizePx(calculated)
}
