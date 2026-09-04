/**
 * Component tests for the settings panel: preset selection and preset save
 * notification. (Target-size input and apply-to-all live in the main view's
 * PresetBar, not here.)
 * 设置面板组件测试：预设选择与保存预设通知。
 * （目标大小输入与「应用到全部」位于主视图的预设条，不在此面板。）
 */
import { beforeEach, describe, expect, it, vi } from 'vite-plus/test'
import { mount } from '@vue/test-utils'

vi.mock('../../config/presets', () => {
  const flags = {
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
    grayscale: false,
    bilevelCodec: 'jpeg',
    subsetFonts: false,
  }
  const profiles = {
    conservative: { imageQuality: 88, maxImageSizePercent: 100, ...flags },
    balanced: { imageQuality: 72, maxImageSizePercent: 80, ...flags },
    maximum: { imageQuality: 58, maxImageSizePercent: 60, ...flags },
    custom: { imageQuality: 72, maxImageSizePercent: 80, ...flags },
  }
  return {
    getDefaultPresetProfiles: vi.fn(() => profiles),
    loadPresetProfiles: vi.fn().mockResolvedValue(profiles),
    hasUserPresetConfig: vi.fn().mockResolvedValue(false),
    saveUserPresetProfile: vi.fn().mockResolvedValue(undefined),
    clearUserPresetConfig: vi.fn().mockResolvedValue(undefined),
  }
})

import {
  clearUserPresetConfig,
  getDefaultPresetProfiles,
  saveUserPresetProfile,
} from '../../config/presets'
import type { CompressionSettings } from '../../types/pdf'
import { i18n } from '../../i18n'
import CompressionSettingsPanel from '../CompressionSettingsPanel.vue'

function makeSettings(overrides: Partial<CompressionSettings> = {}): CompressionSettings {
  return {
    preset: 'balanced',
    imageQuality: 72,
    maxImageSizePercent: 80,
    referenceMaxImageEdgePx: null,
    optimizeImages: true,
    compressStreams: true,
    stripMetadata: true,
    grayscale: false,
    bilevelCodec: 'jpeg',
    subsetFonts: false,
    outputDir: null,
    targetFileSizeMb: null,
    ...overrides,
  }
}

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(CompressionSettingsPanel, {
    props: {
      settings: makeSettings(),
      disabled: false,
      ...props,
    },
    global: {
      plugins: [i18n],
    },
  })
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('CompressionSettingsPanel', () => {
  it('renders the four presets with only the active one checked', () => {
    const wrapper = mountPanel()

    const radios = wrapper.findAll('.preset-ribbon [role="radio"]')
    expect(radios).toHaveLength(4)

    const checked = radios.filter((radio) => radio.attributes('aria-checked') === 'true')
    expect(checked).toHaveLength(1)
    expect(checked[0].text()).toContain('Balanced')
  })

  it('switching a preset emits update:settings with that preset defaults', async () => {
    const wrapper = mountPanel()
    const maximum = wrapper
      .findAll('[role="radio"]')
      .find((radio) => radio.attributes('data-preset-index') === '2')

    expect(maximum).toBeTruthy()
    await maximum!.trigger('click')

    const emitted = wrapper.emitted('update:settings')
    expect(emitted).toBeTruthy()
    const next = emitted![0][0] as CompressionSettings
    expect(next.preset).toBe('maximum')
    expect(next.imageQuality).toBe(58)
    expect(next.maxImageSizePercent).toBe(60)
  })

  it('numeric quality entry commits valid values and rejects out-of-range ones', async () => {
    const wrapper = mountPanel()
    const inputs = wrapper.findAll('.slider-field input')
    expect(inputs).toHaveLength(2)
    const quality = inputs[0]

    ;(quality.element as HTMLInputElement).value = '80'
    await quality.trigger('change')
    const next = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(next.imageQuality).toBe(80)

    ;(quality.element as HTMLInputElement).value = '0'
    await quality.trigger('change')
    expect(quality.attributes('aria-invalid')).toBe('true')
    const count = wrapper.emitted('update:settings')!.length
    ;(quality.element as HTMLInputElement).value = '55'
    await quality.trigger('change')
    const recovered = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(recovered.imageQuality).toBe(55)
    expect(quality.attributes('aria-invalid')).toBeUndefined()
    expect(wrapper.emitted('update:settings')!.length).toBe(count + 1)
  })

  it('numeric size-cap entry commits percent within range', async () => {
    const wrapper = mountPanel()
    const percent = wrapper.findAll('.slider-field input')[1]

    ;(percent.element as HTMLInputElement).value = '50'
    await percent.trigger('change')
    const next = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(next.maxImageSizePercent).toBe(50)

    ;(percent.element as HTMLInputElement).value = '3'
    await percent.trigger('change')
    expect(percent.attributes('aria-invalid')).toBe('true')
  })

  it('saving a preset notifies success; reset stays disabled without changes', async () => {
    const wrapper = mountPanel()
    const saveButton = wrapper.findAll('button').find((button) => button.text() === 'Save preset')

    await saveButton!.trigger('click')
    await vi.waitFor(() => {
      expect(wrapper.emitted('preset-config-saved')).toHaveLength(1)
    })
    expect(saveUserPresetProfile).toHaveBeenCalledWith('balanced', {
      imageQuality: 72,
      maxImageSizePercent: 80,
      optimizeImages: true,
      compressStreams: true,
      stripMetadata: true,
      grayscale: false,
      bilevelCodec: 'jpeg',
      subsetFonts: false,
    })

    const resetButton = wrapper
      .findAll('button')
      .find((button) => button.text() === 'Reset defaults')
    // Settings equal the defaults → reset stays disabled.
    expect((resetButton!.element as HTMLButtonElement).disabled).toBe(true)
    expect(clearUserPresetConfig).not.toHaveBeenCalled()
    expect(getDefaultPresetProfiles).toHaveBeenCalled()
  })
})
