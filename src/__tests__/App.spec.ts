/**
 * App shell wiring smoke test — mounts the real component tree with the
 * Tauri bridge mocked and asserts the main panels render.
 * 应用壳装配冒烟测试 — mock Tauri 桥接后挂载真实组件树，断言主要面板渲染。
 */
import { beforeEach, describe, expect, it, vi } from 'vite-plus/test'
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
  getQuickProfile: vi.fn().mockResolvedValue({ version: 1 }),
  saveQuickProfile: vi.fn().mockImplementation((profile) => Promise.resolve(profile)),
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
  it('renders the header, dropzone and activity rail in the main view', () => {
    const wrapper = mountApp()

    const html = wrapper.html()
    expect(wrapper.find('header.app-header').exists()).toBe(true)
    expect(wrapper.find('section.upload-panel').exists()).toBe(true)
    expect(wrapper.find('section.activity-panel').exists()).toBe(true)
    // The settings panels live on their own view now.
    expect(wrapper.find('section.settings-dock').exists()).toBe(false)
    // No leaked raw i18n keys in the rendered shell.
    expect(html).not.toMatch(/[a-z]+\.[a-zA-Z]+__MISSING/)
  })

  it('switches to the settings view (with quick-mode panel) and back', async () => {
    const wrapper = mountApp()

    await wrapper.find('button.nav-button').trigger('click')
    expect(wrapper.find('section.settings-dock').exists()).toBe(true)
    expect(wrapper.find('section.quick-panel').exists()).toBe(true)
    expect(wrapper.find('section.upload-panel').exists()).toBe(false)

    await wrapper.find('button.nav-button').trigger('click')
    expect(wrapper.find('section.upload-panel').exists()).toBe(true)
    expect(wrapper.find('section.settings-dock').exists()).toBe(false)
  })

  it('shows the empty-queue placeholder in the activity panel', () => {
    const wrapper = mountApp()
    const activity = wrapper.find('section.activity-panel')

    expect(activity.text()).not.toContain('NaN')
    expect(activity.find('[role="progressbar"]').exists()).toBe(true)
  })
})
