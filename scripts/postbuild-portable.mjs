/**
 * Post-build helper for `pnpm run tauri:build`.
 * On Windows, copies the built executable next to the bundle as a portable
 * binary; on other platforms this is a no-op.
 */
import { execSync } from 'node:child_process'
import { copyFileSync } from 'node:fs'

// Resolve the cargo target dir instead of assuming ./target — a machine-wide
// shared target (build.target-dir in ~/.cargo/config.toml) redirects it.
const metadata = JSON.parse(
  execSync('cargo metadata --format-version 1 --no-deps', { encoding: 'utf8' }),
)
const targetDir = metadata.target_directory

if (process.platform === 'win32') {
  copyFileSync(
    `${targetDir}/release/app.exe`,
    `${targetDir}/release/bundle/PDF-Compressor-portable.exe`,
  )
  console.log(`Portable executable copied to ${targetDir}/release/bundle/`)
} else {
  console.log(`Skipping portable copy on ${process.platform}`)
}
