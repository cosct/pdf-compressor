/**
 * Component tests for the error toast viewport: tone roles, dismiss action,
 * and hover/focus pause-resume events feeding the auto-dismiss timers.
 * 错误通知视口的组件测试：级别角色、关闭操作，以及驱动自动消失的悬停/聚焦暂停恢复事件。
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vite-plus/test'

import ErrorToastViewport from '../ErrorToastViewport.vue'
import { i18n } from '../../i18n'
import type { NoticeItem } from '../../types/pdf'

function notice(overrides: Partial<NoticeItem> = {}): NoticeItem {
  return {
    id: 'toast-1',
    tone: 'danger',
    title: 'Compress failed',
    body: 'The backend rejected the file.',
    ...overrides,
  }
}

function mountViewport(items: NoticeItem[]) {
  return mount(ErrorToastViewport, {
    props: { items },
    global: { plugins: [i18n] },
  })
}

describe('ErrorToastViewport', () => {
  it('renders one card per item with tone class and aria role', () => {
    const wrapper = mountViewport([notice(), notice({ id: 'toast-2', tone: 'success' })])

    const cards = wrapper.findAll('.toast-card')
    expect(cards).toHaveLength(2)
    expect(cards[0].classes()).toContain('toast-card--danger')
    expect(cards[0].attributes('role')).toBe('alert')
    expect(cards[1].classes()).toContain('toast-card--success')
    expect(cards[1].attributes('role')).toBe('status')
  })

  it('emits dismiss with the id when the close button is clicked', async () => {
    const wrapper = mountViewport([notice()])

    await wrapper.find('.toast-card__close').trigger('click')

    expect(wrapper.emitted('dismiss')).toEqual([['toast-1']])
  })

  it('emits pause on mouseenter and resume on mouseleave', async () => {
    const wrapper = mountViewport([notice()])
    const card = wrapper.find('.toast-card')

    await card.trigger('mouseenter')
    expect(wrapper.emitted('pause')).toEqual([['toast-1']])
    expect(wrapper.emitted('resume')).toBeUndefined()

    await card.trigger('mouseleave')
    expect(wrapper.emitted('resume')).toEqual([['toast-1']])
  })

  it('emits pause on focusin and resume on focusout', async () => {
    const wrapper = mountViewport([notice()])
    const card = wrapper.find('.toast-card')

    await card.trigger('focusin')
    expect(wrapper.emitted('pause')).toEqual([['toast-1']])

    await card.trigger('focusout')
    expect(wrapper.emitted('resume')).toEqual([['toast-1']])
  })

  it('renders nothing for an empty queue', () => {
    const wrapper = mountViewport([])

    expect(wrapper.find('.toast-viewport').exists()).toBe(false)
  })
})
