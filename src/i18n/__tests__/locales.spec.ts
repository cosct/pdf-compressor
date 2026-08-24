/**
 * Locale parity test — en and zh-CN message catalogs must expose the same
 * key tree so no translation silently falls back at runtime.
 * 语言包一致性测试 — en 与 zh-CN 的 key 树必须一致，避免运行时静默回退。
 */
import { describe, expect, it } from 'vitest'

import en from '../../locales/en'
import zhCN from '../../locales/zh-CN'

function flattenKeys(node: unknown, prefix = ''): string[] {
  if (node === null || typeof node !== 'object') {
    return [prefix.slice(0, -1)]
  }

  return Object.entries(node as Record<string, unknown>).flatMap(([key, value]) =>
    flattenKeys(value, `${prefix}${key}.`),
  )
}

describe('locale catalogs', () => {
  it('en and zh-CN expose identical key trees', () => {
    const enKeys = flattenKeys(en).sort()
    const zhKeys = flattenKeys(zhCN).sort()

    expect(enKeys).toEqual(zhKeys)
  })

  it('plural messages use the pipe syntax in en', () => {
    expect(en.queue.count).toContain('|')
    expect(en.queue.pageCount).toContain('|')
  })
})
