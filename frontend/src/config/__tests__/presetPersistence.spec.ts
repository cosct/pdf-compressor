import { describe, expect, it, vi } from 'vite-plus/test'

vi.mock('../../lib/tauri', () => ({
  hasNativeCommands: () => true,
  loadPresetUserConfig: vi.fn().mockResolvedValue({ version: 3, presets: {} }),
  savePresetUserConfig: vi.fn(),
  clearPresetUserConfig: vi.fn().mockResolvedValue(undefined),
}))

import { savePresetUserConfig } from '../../lib/tauri'
import { saveUserPresetProfile } from '../presets'

describe('preset persistence ordering', () => {
  it('serializes read-modify-write so simultaneous saves preserve both profiles', async () => {
    const save = vi.mocked(savePresetUserConfig)
    let release!: () => void
    save.mockImplementationOnce(
      (config) =>
        new Promise((resolve) => {
          release = () => resolve(config)
        }),
    )
    save.mockImplementation((config) => Promise.resolve(config))
    const first = saveUserPresetProfile('conservative', { imageQuality: 81 })
    const second = saveUserPresetProfile('maximum', { imageQuality: 35 })
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1))
    release()
    await Promise.all([first, second])
    const saved = save.mock.calls[1][0]
    expect(saved.presets.conservative?.imageQuality).toBe(81)
    expect(saved.presets.maximum?.imageQuality).toBe(35)
  })
})
