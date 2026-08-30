/**
 * Single source of truth for the app version: package.json.
 * 在 package.json 中更新版本后运行此脚本，将版本同步到
 * src-tauri/tauri.conf.json 和 src-tauri/Cargo.toml。
 *
 * Usage: npm run sync-version
 */
import { readFileSync, writeFileSync } from 'node:fs'

const packageJson = JSON.parse(readFileSync('package.json', 'utf8'))
const version = packageJson.version

if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version)) {
  throw new Error(`Invalid version in package.json: ${version}`)
}

// --- tauri.conf.json ---
const tauriConfPath = 'src-tauri/tauri.conf.json'
const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'))
if (tauriConf.version !== version) {
  tauriConf.version = version
  writeFileSync(tauriConfPath, `${JSON.stringify(tauriConf, null, 2)}\n`)
  console.log(`${tauriConfPath}: version -> ${version}`)
} else {
  console.log(`${tauriConfPath}: already at ${version}`)
}

// --- Cargo.toml of every workspace crate (only the [package] version line) ---
for (const cargoPath of ['src-tauri/Cargo.toml', 'crates/pdf-core/Cargo.toml']) {
  const cargo = readFileSync(cargoPath, 'utf8')
  const cargoPattern = /^version = ".*"$/m
  if (!cargoPattern.test(cargo)) {
    throw new Error(`No version line found in ${cargoPath}`)
  }
  const updatedCargo = cargo.replace(cargoPattern, `version = "${version}"`)
  if (updatedCargo !== cargo) {
    writeFileSync(cargoPath, updatedCargo)
    console.log(`${cargoPath}: version -> ${version}`)
  } else {
    console.log(`${cargoPath}: already at ${version}`)
  }
}
