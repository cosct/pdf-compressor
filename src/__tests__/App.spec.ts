/**
 * App shell wiring smoke test — mounts the real component tree with the
 * Tauri bridge mocked and asserts the main panels render.
 * 应用壳装配冒烟测试 — mock Tauri 桥接后挂载真实组件树，断言主要面板渲染。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'

vi.mock('../lib/tauri', () => ({
  hasNativeCommands: () => false,
  listenForNativePdfDrop: vi.fn().mockResolvedValue(() => {}),
  listenForOpenPdf: vi.fn().mockResolvedValue(() => {}),
  analyzePdf: vi.fn(),
  compressPdf: vi.fn(),
  cancelCompression: vi.fn(),
  openDirectoryDialog: vi.fn(),
  openPath: vi.fn(),
  openPdfDialog: vi.fn(),
  revealPathInFolder: vi.fn(),
}))

import App from '../App.vue'
import { i18n } from '../i18n'

beforeEach(() => {
  vi.clearAllMocks()
})

function mountApp() {
  return mount(App, {
    global: {
      plugins: [i18n],
    },
  })
}

describe('App', () => {
  it('renders the header, dropzone, settings and activity panels', () => {
    const wrapper = mountApp()

    const html = wrapper.html()
    expect(wrapper.find('header.app-header').exists()).toBe(true)
    expect(wrapper.find('section.upload-panel').exists()).toBe(true)
    expect(wrapper.find('section.settings-dock').exists()).toBe(true)
    expect(wrapper.find('section.activity-panel').exists()).toBe(true)
    // No leaked raw i18n keys in the rendered shell.
    expect(html).not.toMatch(/[a-z]+\.[a-zA-Z]+__MISSING/)
  })

  it('shows the empty-queue placeholder in the activity panel', () => {
    const wrapper = mountApp()
    const activity = wrapper.find('section.activity-panel')

    expect(activity.text()).not.toContain('NaN')
    expect(activity.find('[role="progressbar"]').exists()).toBe(true)
  })
})
