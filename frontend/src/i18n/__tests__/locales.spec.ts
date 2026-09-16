/**
 * Locale parity test — en and zh-CN message catalogs must expose the same
 * key tree so no translation silently falls back at runtime.
 * 语言包一致性测试 — en 与 zh-CN 的 key 树必须一致，避免运行时静默回退。
 */
import { describe, expect, it } from 'vite-plus/test'

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

  // The backend error taxonomy is a wire contract (see error.rs's
  // `error_code_taxonomy_is_a_pinned_closed_set`): every `error.*` code the
  // engine can emit must have a localized body here, and no stale code may
  // linger after a backend change. Additions require a Rust-side addition
  // first; renames are breaking and must touch both sides consciously.
  it('localizes exactly the backend error-code taxonomy', () => {
    const expected = [
      'error.cancelled',
      'error.config',
      'error.encryptedPdf',
      'error.image',
      'error.inputTooLarge',
      'error.invalidPdfPath',
      'error.io',
      'error.missingInput',
      'error.opener',
      'error.passwordRequired',
      'error.pdfBuild',
      'error.wrongPassword',
    ]
    const localized = flattenKeys(en)
      // Leaf paths look like `error.error.cancelled.body` (section prefix +
      // code key + message part); reduce them back to bare codes.
      .filter((key) => key.startsWith('error.error.') && key.endsWith('.body'))
      .map((key) => `error.${key.slice('error.error.'.length, -'.body'.length)}`)
      .sort()

    expect(localized).toEqual([...expected])
  })
})
