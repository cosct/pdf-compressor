/**
 * Post-build helper for `pnpm run tauri:build`.
 * On Windows, copies the built executable next to the bundle as a portable
 * binary; on other platforms this is a no-op.
 */
import { copyFileSync } from 'node:fs'

if (process.platform === 'win32') {
  copyFileSync('target/release/app.exe', 'target/release/bundle/PDF-Compressor-portable.exe')
  console.log('Portable executable copied to target/release/bundle/')
} else {
  console.log(`Skipping portable copy on ${process.platform}`)
}
