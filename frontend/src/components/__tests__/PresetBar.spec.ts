/**
 * Component tests for the main-view preset bar: mode selection (presets and
 * target-size as a peer mode), target-size slider/input/unit controls, color
 * mode, and apply-to-all gating.
 * 主视图预设条组件测试：模式选择（预设与目标大小并列）、目标大小滑块/数值/
 * 单位控制、色彩模式与应用到全部门控。
 */
import { beforeEach, describe, expect, it, vi } from 'vite-plus/test'
import { flushPromises, mount } from '@vue/test-utils'

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
    loadPresetProfiles: vi.fn().mockResolvedValue(profiles),
  }
})

import type { CompressionSettings } from '../../types/pdf'
import { i18n } from '../../i18n'
import PresetBar from '../PresetBar.vue'

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

function mountBar(props: Record<string, unknown> = {}) {
  return mount(PresetBar, {
    props: {
      settings: makeSettings(),
      ...props,
    },
    global: {
      plugins: [i18n],
    },
  })
}

function optionButton(wrapper: ReturnType<typeof mountBar>, text: string) {
  const button = wrapper
    .findAll('.preset-bar__option')
    .find((option) => option.text().includes(text))
  expect(button, `option ${text} must exist`).toBeTruthy()
  return button!
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('PresetBar modes', () => {
  it('switching a preset emits update:settings with that preset', async () => {
    const wrapper = mountBar()
    await flushPromises()
    await optionButton(wrapper, 'Maximum').trigger('click')

    const emitted = wrapper.emitted('update:settings')
    expect(emitted).toBeTruthy()
    const next = emitted![0][0] as CompressionSettings
    expect(next.preset).toBe('maximum')
    expect(next.imageQuality).toBe(58)
  })

  it('selecting a preset mode clears the target size', async () => {
    const wrapper = mountBar({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    await flushPromises()
    await optionButton(wrapper, 'Maximum').trigger('click')

    const next = wrapper.emitted('update:settings')![0][0] as CompressionSettings
    expect(next.preset).toBe('maximum')
    expect(next.targetFileSizeMb).toBeNull()
  })

  it('selecting target-size mode enables it with the default size', async () => {
    const wrapper = mountBar()
    await flushPromises()
    await optionButton(wrapper, 'Target size').trigger('click')

    const next = wrapper.emitted('update:settings')![0][0] as CompressionSettings
    expect(next.targetFileSizeMb).toBe(5)
    expect(next.preset).toBe('balanced')

    // The parent feeds the emitted settings back; controls only render in
    // target mode.
    await wrapper.setProps({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    expect(wrapper.find('.preset-bar__target-controls').exists()).toBe(true)
  })

  it('target controls stay hidden outside target mode', async () => {
    const wrapper = mountBar()
    await flushPromises()
    expect(wrapper.find('.preset-bar__target-controls').exists()).toBe(false)
  })

  it('commits a valid target size entered in the current unit', async () => {
    const wrapper = mountBar({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    const input = wrapper.find('.preset-bar__target-entry input')

    ;(input.element as HTMLInputElement).value = '0.5'
    await input.trigger('change')

    const next = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(next.targetFileSizeMb).toBe(0.5)
  })

  it('rejects an invalid target size with feedback and keeps the value', async () => {
    const wrapper = mountBar({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    const input = wrapper.find('.preset-bar__target-entry input')

    ;(input.element as HTMLInputElement).value = '9999'
    await input.trigger('change')

    expect(wrapper.find('.preset-bar__target-warning').exists()).toBe(true)
    expect(input.attributes('aria-invalid')).toBe('true')
    expect(wrapper.emitted('update:settings')).toBeUndefined()
  })

  it('converts the stored MB value when the unit switches to KB', async () => {
    const wrapper = mountBar({ settings: makeSettings({ targetFileSizeMb: 1 }) })
    const select = wrapper.find('.unit-select__trigger')
    expect(select.text()).toContain('MB')

    const input = wrapper.find('.preset-bar__target-entry input')
    expect((input.element as HTMLInputElement).value).toBe('1')

    await select.trigger('click')
    const kb = wrapper.findAll('.unit-select__option').find((option) => option.text() === 'KB')
    expect(kb).toBeTruthy()
    await kb!.trigger('click')
    expect((input.element as HTMLInputElement).value).toBe('1024')

    // Entering 2048 KB commits 2 MB.
    ;(input.element as HTMLInputElement).value = '2048'
    await input.trigger('change')
    const next = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(next.targetFileSizeMb).toBe(2)
  })

  it('the target entry commits the entered value in the current unit', async () => {
    const wrapper = mountBar({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    const input = wrapper.find('.preset-bar__target-entry input')

    ;(input.element as HTMLInputElement).value = '10'
    await input.trigger('change')

    const next = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(next.targetFileSizeMb).toBe(10)
  })

  it('color mode switches grayscale and the G4 codec together', async () => {
    const wrapper = mountBar()
    const segments = wrapper.findAll('.preset-bar__seg-item')
    expect(segments).toHaveLength(3)

    await segments[2].trigger('click')

    const next = wrapper.emitted('update:settings')![0][0] as CompressionSettings
    expect(next.grayscale).toBe(true)
    expect(next.bilevelCodec).toBe('ccitt-g4')

    await wrapper.findAll('.preset-bar__seg-item')[0].trigger('click')
    const back = wrapper.emitted('update:settings')!.at(-1)![0] as CompressionSettings
    expect(back.grayscale).toBe(false)
    expect(back.bilevelCodec).toBe('jpeg')
  })

  it('read-only params echo the settings and hide in target mode', async () => {
    const wrapper = mountBar()
    await flushPromises()

    const paramValues = () =>
      wrapper.findAll('.preset-bar__param-value').map((param) => param.text())
    expect(paramValues()).toEqual(['72', '80%'])

    // Switching modes adopts the preset profile's values.
    await optionButton(wrapper, 'Maximum').trigger('click')
    await wrapper.setProps({
      settings: makeSettings({ preset: 'maximum', imageQuality: 58, maxImageSizePercent: 60 }),
    })
    expect(paramValues()).toEqual(['58', '60%'])

    // Target mode hides the echo — the engine searches parameters itself.
    await optionButton(wrapper, 'Target size').trigger('click')
    await wrapper.setProps({ settings: makeSettings({ targetFileSizeMb: 5 }) })
    expect(wrapper.find('.preset-bar__params').exists()).toBe(false)
  })

  it('apply-to-all is gated by the flag and emits when enabled', async () => {
    const wrapper = mountBar({ canApplyToAll: false })
    const applyButton = wrapper.find('.preset-bar__apply-all')

    expect(applyButton.exists()).toBe(true)
    expect((applyButton.element as HTMLButtonElement).disabled).toBe(true)

    await wrapper.setProps({ canApplyToAll: true })
    expect((applyButton.element as HTMLButtonElement).disabled).toBe(false)

    await applyButton.trigger('click')
    expect(wrapper.emitted('apply-settings-to-all')).toHaveLength(1)
  })

  it('marks the recommended preset with a tag and no others', async () => {
    const wrapper = mountBar({ recommendedPreset: null })
    await flushPromises()
    expect(wrapper.findAll('.preset-bar__recommend-tag')).toHaveLength(0)

    await wrapper.setProps({ recommendedPreset: 'maximum' })
    const tags = wrapper.findAll('.preset-bar__recommend-tag')
    expect(tags).toHaveLength(1)
    const maximum = wrapper
      .findAll('.preset-bar__option')
      .find((option) => option.text().includes('Maximum'))
    expect(maximum!.find('.preset-bar__recommend-tag').exists()).toBe(true)
  })
})
