/**
 * Error toast state — deduplicated notice queue surfaced via ErrorToastViewport.
 * 错误提示状态 — 去重后的通知队列，由 ErrorToastViewport 呈现。
 */
import { ref } from 'vue'

import type { NoticeItem } from '../types/pdf'
import { normalizeError } from './backendMessages'

export function useErrorToasts() {
  const errorToasts = ref<NoticeItem[]>([])

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

    errorToasts.value = [
      ...errorToasts.value,
      {
        ...notice,
        id: `${notice.id}:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
      },
    ]
  }

  function dismissErrorToast(id: string) {
    errorToasts.value = errorToasts.value.filter((item) => item.id !== id)
  }

  function reportError(error: unknown) {
    pushErrorToast(normalizeError(error))
  }

  return {
    errorToasts,
    pushErrorToast,
    dismissErrorToast,
    reportError,
  }
}
