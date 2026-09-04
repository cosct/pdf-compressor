/**
 * Keyboard-accessibility tests for the queue context menu (WAI-ARIA menu
 * pattern): Menu-key open, arrow navigation, Escape close with focus restore.
 * 队列右键菜单的键盘可达性测试（WAI-ARIA 菜单模式）：Menu 键打开、方向键导航、Escape 关闭并归还焦点。
 */
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vite-plus/test'

import PdfUploadPanel from '../PdfUploadPanel.vue'
import type { QueueVisualItem } from '../PdfUploadPanel.vue'
import { i18n } from '../../i18n'

vi.mock('../../lib/tauri', () => ({
  listenForNativePdfDrop: vi.fn().mockResolvedValue(() => {}),
}))

function queueItem(overrides: Partial<QueueVisualItem> = {}): QueueVisualItem {
  return {
    id: 'job-1',
    fileName: 'report.pdf',
    path: '/tmp/report.pdf',
    status: 'success',
    presetLabel: 'Balanced',
    detail: '',
    meta: [],
    progressPercent: 100,
    outputPath: '/tmp/report__optimized-balanced.pdf',
    ...overrides,
  }
}

function mountPanel(items: QueueVisualItem[]) {
  return mount(PdfUploadPanel, {
    props: {
      items,
      selectedId: null,
      nativeAvailable: false,
      queueLocked: false,
    },
    global: { plugins: [i18n] },
    attachTo: document.body,
  })
}

function pressGlobalEscape() {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
}

async function openMenuWithKeyboard(wrapper: ReturnType<typeof mountPanel>) {
  const item = wrapper.find('.queue-item')
  await item.trigger('keydown', { key: 'ContextMenu' })
  await nextTick()
  await nextTick()
  return item
}

describe('PdfUploadPanel context menu keyboard access', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('opens the menu via the Menu key when the item has an output path', async () => {
    const wrapper = mountPanel([queueItem()])

    await openMenuWithKeyboard(wrapper)

    const menu = wrapper.find('.queue-context-menu')
    expect(menu.exists()).toBe(true)
    expect(menu.attributes('role')).toBe('menu')
    expect(wrapper.emitted('select')).toEqual([['job-1']])
  })

  it('moves focus into the first menu item on open', async () => {
    const wrapper = mountPanel([queueItem()])

    await openMenuWithKeyboard(wrapper)

    const menuItems = wrapper.findAll('[role="menuitem"]')
    expect(menuItems).toHaveLength(2)
    expect(document.activeElement).toBe(menuItems[0].element)
  })

  it('navigates with ArrowDown and wraps around with ArrowUp', async () => {
    const wrapper = mountPanel([queueItem()])

    await openMenuWithKeyboard(wrapper)
    const menu = wrapper.find('.queue-context-menu')
    const menuItems = wrapper.findAll('[role="menuitem"]')

    await menu.trigger('keydown', { key: 'ArrowDown' })
    expect(document.activeElement).toBe(menuItems[1].element)

    await menu.trigger('keydown', { key: 'ArrowDown' })
    expect(document.activeElement).toBe(menuItems[0].element)

    await menu.trigger('keydown', { key: 'ArrowUp' })
    expect(document.activeElement).toBe(menuItems[1].element)
  })

  it('closes on Escape and restores focus to the queue item', async () => {
    const wrapper = mountPanel([queueItem()])
    const item = await openMenuWithKeyboard(wrapper)
    const triggerElement = item.element as HTMLElement
    expect(document.activeElement).not.toBe(triggerElement)

    pressGlobalEscape()
    await nextTick()

    expect(wrapper.find('.queue-context-menu').exists()).toBe(false)
    expect(document.activeElement).toBe(triggerElement)
  })

  it('does not open the menu for items without an output path', async () => {
    const wrapper = mountPanel([queueItem({ outputPath: null, status: 'ready' })])

    const item = wrapper.find('.queue-item')
    await item.trigger('keydown', { key: 'ContextMenu' })
    await nextTick()

    expect(wrapper.find('.queue-context-menu').exists()).toBe(false)
  })

  it('opens at the item via Shift+F10 as well', async () => {
    const wrapper = mountPanel([queueItem()])

    const item = wrapper.find('.queue-item')
    await item.trigger('keydown', { key: 'F10', shiftKey: true })
    await nextTick()
    await nextTick()

    expect(wrapper.find('.queue-context-menu').exists()).toBe(true)
  })

  it('emits select on Enter and delete on the delete button', async () => {
    const wrapper = mountPanel([queueItem()])

    const item = wrapper.find('.queue-item')
    await item.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('select')).toEqual([['job-1']])

    await wrapper.find('.queue-item__delete').trigger('click')
    expect(wrapper.emitted('delete')).toEqual([['job-1']])
  })
})
