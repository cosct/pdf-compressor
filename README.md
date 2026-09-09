# PDF Compressor

[![CI](https://github.com/cosct/pdf-compressor/actions/workflows/ci.yml/badge.svg)](https://github.com/cosct/pdf-compressor/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/cosct/pdf-compressor.svg)](https://github.com/cosct/pdf-compressor/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

English | [简体中文](README.zh-CN.md)

Make PDFs smaller **without breaking them**: text stays searchable and copyable, images get re-encoded to your quality target, everything else is left untouched. All processing happens on your own machine — no uploads, no accounts, no network required.

![Main window](docs/screenshots/app-main.png)

## What it does

- **Touches only what matters**: an analysis pass figures out where the bytes actually go (images? embedded fonts? leftover cruft?) and optimizes just that; text and line art are never converted to pictures
- **Serious image compression**: JPEG re-encoding at your target quality, automatic downsampling of oversized images, color-to-grayscale; black-and-white scans use the lossless fax codec (G4) and often shrink to a fraction of their size
- **Budget for your attachments**: target-size mode takes "under 5 MB" and searches the quality/resolution combinations until one fits — spending budget wisely, less on text pages and more on photo pages
- **Cleans up invisible bloat**: byte-identical duplicates are merged into one, unused resources left behind by editors are removed, and embedded fonts are trimmed to the characters actually used (a big win for CJK documents)
- **Plays it safe**: an output that wouldn't beat the original is never written; password-protected files follow an explicit flow; your original file is never overwritten
- **Fully offline**: your files never leave your machine, and nothing is tracked
- **Fits your workflow**: multi-file queue in the GUI, one-liner CLI for batches, and right-click integration in Windows Explorer, macOS Finder, KDE Dolphin, and GNOME Files

## Install

| Platform | How |
| --- | --- |
| Windows | Grab the installer (`.exe`) from [Releases](https://github.com/cosct/pdf-compressor/releases) — ships the CLI and an Explorer context menu |
| macOS | Grab the `.dmg` from [Releases](https://github.com/cosct/pdf-compressor/releases) (universal, Intel / Apple Silicon) |
| Arch Linux | AUR: `yay -S pdf-compressor-bin` (binary) or `yay -S pdf-compressor` (source) |
| Other Linux | Grab the `.AppImage` or `.deb` from [Releases](https://github.com/cosct/pdf-compressor/releases) |
| From source | `pnpm install && pnpm run tauri build` (Rust 1.93+, Node.js 22+) |

## Quick start

**GUI** — drag PDFs in, glance at the analysis (expected savings and a recommended preset), press *Start compression*. Results land next to the originals; the original file is never touched.

**CLI** —

```bash
pdf-compressor-cli analyze input.pdf                          # see where the bytes go
pdf-compressor-cli compress input.pdf --preset maximum        # compress (aggressive)
pdf-compressor-cli compress input.pdf --target-size 5MB       # fit under a budget
pdf-compressor-cli quick scans/ --no-notify                   # batch a directory, headless
curl -s https://example.com/big.pdf | pdf-compressor-cli compress - --stdout > small.pdf
                                                              # drop into a pipe, no temp files
```

The CLI drives the same engine as the GUI with identical parameter semantics — see the [user guide](docs/USER-GUIDE.zh-CN.md) (中文) for the full reference.

## How it works (one-minute version)

A PDF is usually big because images are stored larger than needed, whole fonts are embedded, or years of edits left redundant data behind. This tool inspects each of those and reorganizes them to **shrink bytes without changing appearance**: images are re-encoded to the target quality, duplicates merged, unused font data trimmed. Text, bookmarks, links, and page structure are always preserved — so the compressed file still searches, copies, and prints like the original.

Curious what each setting means and the trade-offs behind it? Read the [settings walkthrough](docs/USER-GUIDE.zh-CN.md#3-压缩参数详解) in the user guide.

## Documentation

| Document | Audience |
| --- | --- |
| [User guide (中文)](docs/USER-GUIDE.zh-CN.md) | Everyone — install, GUI & CLI usage, context menus, password handling, FAQ, glossary |
| [Changelog](CHANGELOG.md) | Everyone — release history |
| [Development guide](docs/DEVELOPMENT.md) | Contributors — architecture, conventions, packaging, maintainer notes |
| [Testing guide](docs/TESTING.md) | Contributors / QA — test pyramid and quality gates |

## Contributing

Issues and pull requests are welcome. See the [development guide](docs/DEVELOPMENT.md) for environment setup, code layout, and conventions — please run the local quality gate (`pnpm gate`) before opening a PR.

## License

[MIT](LICENSE)
