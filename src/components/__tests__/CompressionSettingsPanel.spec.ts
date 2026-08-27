/**
 * Component tests for the settings panel: preset selection, apply-to-all
 * gating, target-size validation feedback, and preset save notification.
 * 设置面板组件测试：预设选择、应用到全部的门控、目标大小校验反馈与保存预设通知。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'

vi.mock('../../config/presets', () => {
  const profiles = {
    conservative: { imageQuality: 88, maxImageSizePercent: 100 },
    balanced: { imageQuality: 72, maxImageSizePercent: 80 },
    maximum: { imageQuality: 58, maxImageSizePercent: 60 },
    custom: { imageQuality: 72, maxImageSizePercent: 80 },
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
      recommendedPreset: null,
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

  it('selecting black & white switches grayscale and the G4 codec together', async () => {
    const wrapper = mountPanel()
    const colorModeGroup = wrapper.findAll('.color-mode-seg__item')
    expect(colorModeGroup).toHaveLength(3)

    await colorModeGroup[2].trigger('click')

    const emitted = wrapper.emitted('update:settings')
    expect(emitted).toBeTruthy()
    const next = emitted![0][0] as CompressionSettings
    expect(next.grayscale).toBe(true)
    expect(next.bilevelCodec).toBe('ccitt-g4')
  })

  it('apply-to-all is disabled without the flag and emits when enabled', async () => {
    const wrapper = mountPanel({ canApplyToAll: false })
    const applyButton = wrapper
      .findAll('button')
      .find((button) => button.text() === 'Apply to all')

    expect(applyButton).toBeTruthy()
    expect((applyButton!.element as HTMLButtonElement).disabled).toBe(true)

    await wrapper.setProps({ canApplyToAll: true })
    expect((applyButton!.element as HTMLButtonElement).disabled).toBe(false)

    await applyButton!.trigger('click')
    expect(wrapper.emitted('apply-settings-to-all')).toHaveLength(1)
  })

  it('rejects an invalid target size with visible feedback, then accepts a valid one', async () => {
    const wrapper = mountPanel()
    const input = wrapper.find('.target-size-input')

    // Assign the element value directly: `setValue` routes through happy-dom's
    // value sanitizer differently for number inputs. `0` parses as a number
    // but fails the > 0 rule.
    ;(input.element as HTMLInputElement).value = '0'
    await input.trigger('change')
    await nextTick()

    expect(wrapper.find('.target-size-warning').exists()).toBe(true)
    expect(input.attributes('aria-invalid')).toBe('true')
    const rejected = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(rejected.targetFileSizeMb).toBeNull()

    ;(input.element as HTMLInputElement).value = '5'
    await input.trigger('change')
    await nextTick()

    expect(wrapper.find('.target-size-warning').exists()).toBe(false)
    expect(input.attributes('aria-invalid')).toBeUndefined()
    const accepted = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(accepted.targetFileSizeMb).toBe(5)
  })

  it('saving a preset notifies success; reset stays disabled without changes', async () => {
    const wrapper = mountPanel()
    const saveButton = wrapper
      .findAll('button')
      .find((button) => button.text() === 'Save preset')

    await saveButton!.trigger('click')
    await vi.waitFor(() => {
      expect(wrapper.emitted('preset-config-saved')).toHaveLength(1)
    })
    expect(saveUserPresetProfile).toHaveBeenCalledWith('balanced', {
      imageQuality: 72,
      maxImageSizePercent: 80,
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
