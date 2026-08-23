/**
 * Error toast state — deduplicated notice queue surfaced via ErrorToastViewport.
 * 错误提示状态 — 去重后的通知队列，由 ErrorToastViewport 呈现。
 *
 * Toasts auto-dismiss after a tone-based timeout that scales with message
 * length; hovering or focusing a toast pauses its timer so the text stays
 * readable and the close button remains reachable (WCAG 2.2.1).
 */
import { onScopeDispose, ref } from 'vue'

import type { NoticeItem } from '../types/pdf'
import { normalizeError } from './backendMessages'

/** Base auto-dismiss delay per tone; danger errors linger the longest. */
const AUTO_DISMISS_BASE_MS: Record<NoticeItem['tone'], number> = {
  danger: 10_000,
  warning: 6_000,
  success: 4_000,
  neutral: 6_000,
}

/** Extra delay per character of the longest text run, capped below. */
const AUTO_DISMISS_MS_PER_CHAR = 60
const AUTO_DISMISS_MAX_MS = 20_000

interface ToastTimer {
  deadline: number
  remaining: number
  handle: number | null
}

export function useErrorToasts() {
  const errorToasts = ref<NoticeItem[]>([])
  const timers = new Map<string, ToastTimer>()

  function autoDismissDelayMs(notice: NoticeItem): number {
    const base = AUTO_DISMISS_BASE_MS[notice.tone]
    const lengthBonus =
      Math.max(notice.title.length, notice.body.length) * AUTO_DISMISS_MS_PER_CHAR
    return Math.min(base + lengthBonus, AUTO_DISMISS_MAX_MS)
  }

  function clearTimer(id: string) {
    const timer = timers.get(id)
    if (timer?.handle !== null && timer?.handle !== undefined) {
      window.clearTimeout(timer.handle)
    }
    timers.delete(id)
  }

  function armTimer(id: string, delayMs: number) {
    clearTimer(id)
    const handle = window.setTimeout(() => {
      timers.delete(id)
      dismissErrorToast(id)
    }, delayMs)
    timers.set(id, { deadline: Date.now() + delayMs, remaining: delayMs, handle })
  }

  function pushErrorToast(notice: NoticeItem) {
    const duplicate = errorToasts.value.some(
      (item) =>
        item.tone === notice.tone &&
        item.title === notice.title &&
        item.body === notice.body,
    )
    if (duplicate) {
      return
    }

    const enriched: NoticeItem = {
      ...notice,
      id: `${notice.id}:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
    }

    errorToasts.value = [...errorToasts.value, enriched]
    armTimer(enriched.id, autoDismissDelayMs(enriched))
  }

  function dismissErrorToast(id: string) {
    clearTimer(id)
    errorToasts.value = errorToasts.value.filter((item) => item.id !== id)
  }

  function pauseErrorToast(id: string) {
    const timer = timers.get(id)
    if (!timer || timer.handle === null) {
      return
    }

    window.clearTimeout(timer.handle)
    timer.remaining = Math.max(timer.deadline - Date.now(), 0)
    timer.handle = null
  }

  function resumeErrorToast(id: string) {
    const timer = timers.get(id)
    if (!timer || timer.handle !== null) {
      return
    }

    if (timer.remaining <= 0) {
      dismissErrorToast(id)
      return
    }

    armTimer(id, timer.remaining)
  }

  function reportError(error: unknown) {
    pushErrorToast(normalizeError(error))
  }

  onScopeDispose(() => {
    for (const id of [...timers.keys()]) {
      clearTimer(id)
    }
  })

  return {
    errorToasts,
    pushErrorToast,
    dismissErrorToast,
    pauseErrorToast,
    resumeErrorToast,
    reportError,
  }
}
