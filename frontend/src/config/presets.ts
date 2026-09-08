/**
 * Preset profile management — load, save, clear, and merge compression presets.
 * 预设配置管理 — 加载、保存、清除和合并压缩预设。
 *
 * Built-in defaults come from preset-defaults.json. User-saved overrides are
 * persisted via Rust backend commands and cached in memory.
 * 内置默认值来自 preset-defaults.json。用户保存的覆盖通过 Rust 后端命令持久化，并在内存中缓存。
 */
import presetDefaultsJson from './preset-defaults.json'

import {
  clearPresetUserConfig as clearNativePresetUserConfig,
  hasNativeCommands,
  loadPresetUserConfig as loadNativePresetUserConfig,
  savePresetUserConfig as saveNativePresetUserConfig,
} from '../lib/tauri'
import type {
  CompressionPreset,
  PresetDefaults,
  PresetProfile,
  PresetProfileMap,
  PresetUserConfig,
} from '../types/pdf'
import { clampImageQuality, clampMaxImageSizePercent } from '../utils/compressionSettings'

// v2 (0.8.0): cmykConversion default flip — v1 configs get the load-time
// migration in the backend; configs created here are current-generation.
const CONFIG_VERSION = 2

const PRESET_KEYS: CompressionPreset[] = ['conservative', 'balanced', 'maximum', 'custom']

/** Presets with their own entry in preset-defaults.json. */
const BUILTIN_DEFAULT_PRESETS: Exclude<CompressionPreset, 'custom'>[] = [
  'conservative',
  'balanced',
  'maximum',
]

let cachedUserPresetConfig: PresetUserConfig | null = null
let pendingUserPresetConfigLoad: Promise<PresetUserConfig> | null = null

function clonePresetProfile(profile: PresetProfile): PresetProfile {
  return { ...profile }
}

/** Flag/codec defaults for preset files that predate the extended profile
 * shape (only imageQuality + maxImageSizePercent used to be persisted). */
const LEGACY_PROFILE_FALLBACK = {
  optimizeImages: true,
  compressStreams: true,
  stripMetadata: true,
  grayscale: false,
  bilevelCodec: 'jpeg',
  subsetFonts: false,
  cmykConversion: true,
} as const

function sanitizePresetProfile(
  profile: Partial<PresetProfile> | undefined,
  fallback: PresetProfile,
): PresetProfile {
  return {
    imageQuality: clampImageQuality(profile?.imageQuality ?? fallback.imageQuality),
    maxImageSizePercent: clampMaxImageSizePercent(
      profile?.maxImageSizePercent ?? fallback.maxImageSizePercent,
    ),
    optimizeImages:
      profile?.optimizeImages ?? fallback.optimizeImages ?? LEGACY_PROFILE_FALLBACK.optimizeImages,
    compressStreams:
      profile?.compressStreams ??
      fallback.compressStreams ??
      LEGACY_PROFILE_FALLBACK.compressStreams,
    stripMetadata:
      profile?.stripMetadata ?? fallback.stripMetadata ?? LEGACY_PROFILE_FALLBACK.stripMetadata,
    grayscale: profile?.grayscale ?? fallback.grayscale ?? LEGACY_PROFILE_FALLBACK.grayscale,
    bilevelCodec:
      profile?.bilevelCodec === 'ccitt-g4'
        ? 'ccitt-g4'
        : (fallback.bilevelCodec ?? LEGACY_PROFILE_FALLBACK.bilevelCodec),
    subsetFonts:
      profile?.subsetFonts ?? fallback.subsetFonts ?? LEGACY_PROFILE_FALLBACK.subsetFonts,
    cmykConversion:
      profile?.cmykConversion ?? fallback.cmykConversion ?? LEGACY_PROFILE_FALLBACK.cmykConversion,
  }
}

const defaultPresetProfiles = BUILTIN_DEFAULT_PRESETS.reduce((profiles, preset) => {
  profiles[preset] = sanitizePresetProfile(
    presetDefaultsJson[preset] as Partial<PresetProfile>,
    presetDefaultsJson[preset] as PresetProfile,
  )
  return profiles
}, {} as PresetProfileMap)

// `custom` has no JSON entry of its own: hand-tuned settings start from a
// clone of `maximum`, the most aggressive baseline.
defaultPresetProfiles.custom = clonePresetProfile(defaultPresetProfiles.maximum)

function createEmptyUserConfig(): PresetUserConfig {
  return {
    version: CONFIG_VERSION,
    presets: {},
  }
}

function cloneUserPresetConfig(config: PresetUserConfig): PresetUserConfig {
  const presets: PresetUserConfig['presets'] = {}

  for (const preset of PRESET_KEYS) {
    const profile = config.presets[preset]
    if (profile) {
      presets[preset] = clonePresetProfile(profile)
    }
  }

  return {
    version: config.version,
    presets,
  }
}

function sanitizeUserPresetConfig(
  input: Partial<PresetUserConfig> | null | undefined,
): PresetUserConfig {
  const config = createEmptyUserConfig()
  config.version = Number.isFinite(input?.version)
    ? Math.max(1, Math.round(input?.version ?? 1))
    : CONFIG_VERSION

  for (const preset of PRESET_KEYS) {
    const profile = input?.presets?.[preset]
    if (profile) {
      config.presets[preset] = sanitizePresetProfile(profile, defaultPresetProfiles[preset])
    }
  }

  return config
}

function buildPresetProfiles(config: PresetUserConfig | null): PresetProfileMap {
  const profiles = getDefaultPresetProfiles()

  if (!config) {
    return profiles
  }

  for (const preset of PRESET_KEYS) {
    const profile = config.presets[preset]
    if (profile) {
      profiles[preset] = clonePresetProfile(profile)
    }
  }

  return profiles
}

async function ensureUserPresetConfigLoaded(forceReload = false): Promise<PresetUserConfig> {
  if (!forceReload && cachedUserPresetConfig) {
    return cloneUserPresetConfig(cachedUserPresetConfig)
  }

  if (!forceReload && pendingUserPresetConfigLoad) {
    return pendingUserPresetConfigLoad.then((config) => cloneUserPresetConfig(config))
  }

  pendingUserPresetConfigLoad = (async () => {
    if (!hasNativeCommands()) {
      cachedUserPresetConfig = createEmptyUserConfig()
      return cachedUserPresetConfig
    }

    const loadedConfig = await loadNativePresetUserConfig()
    cachedUserPresetConfig = sanitizeUserPresetConfig(loadedConfig)
    return cachedUserPresetConfig
  })().finally(() => {
    pendingUserPresetConfigLoad = null
  })

  const config = await pendingUserPresetConfigLoad
  return cloneUserPresetConfig(config)
}

export function getDefaultPresetProfiles(): PresetProfileMap {
  return PRESET_KEYS.reduce((profiles, preset) => {
    profiles[preset] = clonePresetProfile(defaultPresetProfiles[preset])
    return profiles
  }, {} as PresetProfileMap)
}

export async function loadPresetProfiles(): Promise<PresetProfileMap> {
  const userConfig = await ensureUserPresetConfigLoaded()
  return buildPresetProfiles(userConfig)
}

export async function hasUserPresetConfig(): Promise<boolean> {
  const config = await ensureUserPresetConfigLoaded()
  return PRESET_KEYS.some((preset) => Boolean(config.presets[preset]))
}

export function getPresetDefaults(preset: CompressionPreset): PresetDefaults {
  const profile = cachedUserPresetConfig?.presets[preset] ?? defaultPresetProfiles[preset]
  return { ...profile }
}

export async function saveUserPresetProfile(
  preset: CompressionPreset,
  profile: Partial<PresetProfile>,
): Promise<void> {
  const config = await ensureUserPresetConfigLoaded()
  const nextConfig = sanitizeUserPresetConfig({
    ...config,
    presets: {
      ...config.presets,
      [preset]: sanitizePresetProfile(profile, defaultPresetProfiles[preset]),
    },
  })

  if (hasNativeCommands()) {
    const savedConfig = await saveNativePresetUserConfig(nextConfig)
    cachedUserPresetConfig = sanitizeUserPresetConfig(savedConfig)
    return
  }

  cachedUserPresetConfig = nextConfig
}

export async function clearUserPresetConfig(): Promise<void> {
  if (hasNativeCommands()) {
    await clearNativePresetUserConfig()
  }

  cachedUserPresetConfig = createEmptyUserConfig()
}
