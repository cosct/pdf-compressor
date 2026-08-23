/**
 * Auto-dismiss timer tests for the error toast state: tone-based timeouts,
 * reading-time scaling, and pause/resume behavior.
 * 错误提示状态的自动消失计时测试：分级超时、按文本长度延长、暂停/恢复行为。
 */
import { effectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useErrorToasts } from '../useErrorToasts'
import type { NoticeItem } from '../../types/pdf'

function notice(overrides: Partial<NoticeItem> = {}): NoticeItem {
  return {
    id: 'toast',
    tone: 'danger',
    title: 'Failed',
    body: 'Short body.',
    ...overrides,
  }
}

describe('useErrorToasts auto-dismiss', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  function setup() {
    const scope = effectScope()
    const store = scope.run(() => useErrorToasts())!
    return {
      store,
      dispose: () => scope.stop(),
    }
  }

  it('removes a success toast after its (length-extended) delay', () => {
    const { store } = setup()
    store.pushErrorToast(notice({ tone: 'success' }))

    // Base 4s + 60ms/char ("Short body." = 12 chars → 720ms) ≈ 4.72s.
    vi.advanceTimersByTime(4_500)
    expect(store.errorToasts.value).toHaveLength(1)

    vi.advanceTimersByTime(500)
    expect(store.errorToasts.value).toHaveLength(0)
  })

  it('keeps danger toasts noticeably longer than success toasts', () => {
    const { store } = setup()
    const body = 'Short body.'
    store.pushErrorToast(notice({ tone: 'danger', body }))
    store.pushErrorToast(notice({ id: 'other', tone: 'success', body }))

    // danger ≈ 10s + 0.72s length bonus; success ≈ 4s + 0.72s.
    vi.advanceTimersByTime(6_000)
    expect(store.errorToasts.value.map((item) => item.tone)).toEqual(['danger'])
  })

  it('extends the timeout for longer message bodies', () => {
    const { store } = setup()
    store.pushErrorToast(notice({ tone: 'success', title: 'T', body: 'x'.repeat(100) }))

    vi.advanceTimersByTime(6_000)
    expect(store.errorToasts.value).toHaveLength(1)

    // 4s base + 100 chars × 60ms = 10s total.
    vi.advanceTimersByTime(4_500)
    expect(store.errorToasts.value).toHaveLength(0)
  })

  it('pause stops the timer and resume finishes the remaining time', () => {
    const { store } = setup()
    store.pushErrorToast(notice({ tone: 'success', title: 'T', body: '' }))
    const id = store.errorToasts.value[0]!.id

    vi.advanceTimersByTime(2_000)
    store.pauseErrorToast(id)
    vi.advanceTimersByTime(60_000)
    expect(store.errorToasts.value).toHaveLength(1)

    store.resumeErrorToast(id)
    vi.advanceTimersByTime(1_500)
    expect(store.errorToasts.value).toHaveLength(1)

    vi.advanceTimersByTime(1_000)
    expect(store.errorToasts.value).toHaveLength(0)
  })

  it('manual dismissal clears the pending timer', () => {
    const { store } = setup()
    store.pushErrorToast(notice({ tone: 'danger' }))
    const id = store.errorToasts.value[0]!.id

    store.dismissErrorToast(id)
    expect(store.errorToasts.value).toHaveLength(0)

    vi.advanceTimersByTime(60_000)
    expect(store.errorToasts.value).toHaveLength(0)
  })

  it('ignores duplicate notices regardless of generated id', () => {
    const { store } = setup()
    store.pushErrorToast(notice())
    store.pushErrorToast(notice())

    expect(store.errorToasts.value).toHaveLength(1)
  })
})
