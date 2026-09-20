/**
 * Preset-profile sanitizer pins: the wire-level defaults and the
 * round-tripping of explicit user choices.
 * 预设档案清洗器钉子：线格式默认值与用户显式选择的往返保持。
 */
import { describe, expect, it } from 'vite-plus/test'

import { CONFIG_VERSION, sanitizePresetProfile } from '../presets'

function fallback() {
  return {
    imageQuality: 72,
    maxImageSizePercent: 80,
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
    grayscale: false,
    bilevelCodec: 'ccitt-g4' as const,
    subsetFonts: false,
    cmykConversion: true,
  }
}

describe('sanitizePresetProfile', () => {
  it('preserves an explicit jpeg bilevel codec instead of rewriting it to the default', () => {
    // Regression (0.11.0): the sanitizer only recognized 'ccitt-g4' and
    // silently rewrote a saved 'jpeg' opt-out back to the G4 default on
    // both save and load — while the queue-side sanitizer kept it.
    const profile = sanitizePresetProfile({ ...fallback(), bilevelCodec: 'jpeg' }, fallback())
    expect(profile.bilevelCodec).toBe('jpeg')
  })

  it('falls back to the preset default for unrecognized codec values', () => {
    const profile = sanitizePresetProfile(
      { ...fallback(), bilevelCodec: 'av1' as unknown as 'jpeg' },
      fallback(),
    )
    expect(profile.bilevelCodec).toBe('ccitt-g4')
  })

  it('keeps the config version in lockstep with the backend migration chain', () => {
    // v3 is the bilevel default flip (0.10.0); a stale version here would
    // have configs the backend re-migrates and strips explicit choices from.
    expect(CONFIG_VERSION).toBe(3)
  })
})
