# PDF Compressor

Language versions: `README.md` (English) | `README.zh-CN.md` (简体中文)

PDF Compressor is a local-first desktop app for reducing PDF size with a Rust backend and a Vue 3 + Tauri frontend. It supports a multi-file queue workflow: add one or more PDFs, let the app analyze each file, adjust settings per file or globally, and export lighter copies without overwriting the originals.

Current release: `0.2.0`

Author: `cosct`

## Overview

This project is designed for selective PDF optimization rather than blind whole-file rewriting. The current pipeline tries to preserve text and vector instructions whenever possible, then reduces size by working on areas that are usually safer to optimize:

- Embedded image streams can be recompressed as JPEG and resized when needed
- Images carrying transparency (`/SMask`) are rewritten with their alpha plane preserved
- Byte-identical duplicate images (logos, stamps) are losslessly merged into shared references
- A target-size mode searches quality/resolution parameters until the output fits a byte budget (UI, CLI `--target-size`, and IPC)
- Eligible non-image PDF streams can be compressed
- Document metadata can be removed
- The UI runs an analysis pass first so the user can review likely savings and a suggested preset before export

The application is desktop-first and local-first. There is no upload flow and no cloud processing. The desktop release supports a local queue so multiple PDFs can be analyzed and compressed in batch.

## Installation

### Arch Linux (AUR)

AUR source-package files are provided in [`aur/pdf-compressor/`](aur/pdf-compressor/). Once the package is published to the AUR, it can be installed with any AUR helper:

```bash
yay -S pdf-compressor      # or: paru -S pdf-compressor
```

To build the package locally from the provided files, follow [`aur/pdf-compressor/README.md`](aur/pdf-compressor/README.md). It covers replacing the source checksum, regenerating `.SRCINFO`, and creating a local source tarball for testing before a release archive is published.

### Other platforms

No prebuilt installers are published yet. Build from source as described in [Build and Release](#build-and-release). On Windows this produces an NSIS installer and a portable executable.

## What's New in v0.2.0

### Fluent Design UI

The entire interface has been redesigned using Microsoft Fluent Design System principles:

- **Dark and light themes** — The app supports three theme modes: Dark, Light, and System (follows OS preference). System is the default on first launch. Theme selection is persisted in local storage and applied before first paint to avoid visual flash.
- **Acrylic surfaces** — The header uses an acrylic backdrop-filter effect (blurred, semi-transparent) for depth layering, consistent with Windows 11 design language.
- **Fluent tokens** — All colors, spacing, typography, border radii, shadows, and transitions are defined as CSS custom properties following the WinUI 3 / Fluent 2 token specification. Dark and light themes each have a complete set of semantic tokens.
- **PDF upload as primary view** — The upload/queue panel is now the dominant left-side area, taking up the majority of screen real estate. Settings and activity are placed in a narrower right sidebar, making the drag-and-drop intake area the clear hero element.
- **Responsive layout** — The two-column grid collapses to a single column on narrow viewports. The sidebar uses sticky positioning on wide screens so it stays visible while scrolling through a long queue.
- **Custom window chrome** — The app uses a frameless window with an integrated custom titlebar that includes minimize, maximize/restore, and close controls alongside the theme and locale switchers.
- **Consistent iconography** — Each panel header has a small inline SVG icon for visual anchoring. The drop zone features a larger upload icon for discoverability.
- **Toggle switches** — Boolean settings (optimize images, compress streams, strip metadata) use Fluent-style toggle switches instead of raw checkboxes.
- **Splash screen** — A lightweight splash window is displayed during initial load while the main window and Vue app initialize, then dismissed via `app_ready`.
- **Error toasts** — Backend errors and dialog failures are surfaced as floating toast cards with per-tone styling (danger, warning, success) and a dismiss button.

### Compression Speed Optimization

The Rust compression engine received targeted performance improvements:

- **CatmullRom resize filter** — Replaced `Triangle` with `CatmullRom` (bicubic interpolation) for the final resize pass. CatmullRom is approximately 2× faster than `Lanczos3` with nearly indistinguishable quality for JPEG-bound output, and sharper than `Triangle`.
- **Earlier two-pass threshold** — The two-pass resize strategy (Nearest then CatmullRom) now triggers at 4 million pixels instead of 8 million. This means moderately large images (e.g. 2000×2000) benefit from the fast first pass, reducing total resize time.
- **Wider edge tolerance** — The `RESIZE_EDGE_TOLERANCE` was increased from 1.05 to 1.08. Images that are only slightly over the target edge are no longer resized, avoiding a decode+resize+encode round-trip for negligible dimension reduction.
- **Larger worker channel buffer** — The parallel image processing channel buffer is now 4× the worker count (up from 2×), reducing blocking on the producer thread and improving pipeline throughput.
- **Lower parallel threshold** — Parallel image processing now triggers with 3+ images (down from 4), allowing smaller PDFs to benefit from multi-core processing.
- **Smaller stream compression minimum** — Non-image streams of 64+ bytes (down from 128) are now candidates for deflate compression, catching more short repetitive streams.
- **Lower tiny-JPEG skip threshold** — JPEG streams under 6 KB (down from 8 KB) are skipped outright, applying fast-path exits more aggressively on truly tiny images.
- **Higher small-stream threshold** — The `SMALL_IMAGE_STREAM_BYTES` threshold was raised to 64 KB (from 48 KB), allowing more compact JPEGs to be skipped when they are already within the target dimensions.
- **FlateDecode detection** — The stream filter analysis now tracks `FlateDecode` presence, enabling future optimizations for already-deflated streams.

### Theme System Architecture

The theme system is implemented as a Vue composable (`src/composables/useTheme.ts`):

- **Reactive state** — `themePreference` (ref) tracks the user's choice: `'dark'`, `'light'`, or `'system'`. `resolvedTheme` (computed) resolves `'system'` to the actual OS preference.
- **System default** — On first launch with no stored preference, the theme defaults to `'system'`, which follows the OS dark/light setting.
- **DOM synchronization** — A watcher applies `data-theme` attribute and `color-scheme` CSS property to `<html>` whenever the resolved theme changes.
- **System preference listening** — The composable listens for `prefers-color-scheme` media query changes, so switching OS theme while the app is open updates the UI instantly when mode is set to System.
- **Flash prevention** — A synchronous `<script>` block in `index.html` reads the stored theme from localStorage and applies the `data-theme` attribute before any CSS or Vue code loads. A CSS `prefers-color-scheme` media query provides the fallback before the script executes.
- **Persistence** — Theme preference is stored in localStorage under `pdf-compressor-theme`.

### Preset Persistence

User-customized preset profiles are persisted to disk via `src/config/presets.ts` and the Rust `commands.rs` command surface:

- **Save location** — The backend tries the install directory first; falls back to the OS application config directory (e.g. `AppData/Roaming/pdf-compressor`) when the install directory is not writable (e.g. `Program Files`).
- **Atomic writes** — Config is written to a `.tmp` file then renamed, avoiding corruption from interrupted writes.
- **Frontend cache** — Loaded config is cached in memory to avoid repeated disk reads during a session.
- **Reset** — Clearing user overrides removes the config file and restores built-in defaults.

## Main Features

### User-facing features

- Multi-file PDF queue with drag-and-drop and native desktop browse
- Automatic preflight analysis before compression
- Dark, light, and system theme — defaults to system preference on first launch
- Three presets: `conservative`, `balanced`, `maximum`, plus `custom`
- Adjustable image quality and maximum image edge (percentage-based with reference edge)
- Toggles for image optimization, stream compression, and metadata removal
- Custom preset saving, per-preset user overrides, and reset to defaults
- Apply settings to all queued files at once
- Selectable output directory for compressed files
- Queue-aware activity view with output path, progress, size delta, and optimization counts
- Open or reveal compressed output files via system handler
- Compression cancellation with per-task tracking
- Error toast notifications for backend and dialog failures
- Splash screen during initial load
- Custom window chrome (frameless with integrated titlebar controls)
- Portable executable alongside NSIS installer in production build
- English and Simplified Chinese UI

### Backend features

- PDF structure inspection through `lopdf`
- Pure-Rust preflight analysis based on page maps, page resources, and extractable text
- Heuristic document classification: `text-native`, `mixed`, or `scan-heavy`
- Suggested preset based on scanned-document confidence
- Sampled page inspection for large PDFs so recommendations stay responsive
- Safe-skip behavior for image streams that are not yet supported for rewriting
- Parallel image recompression with optimized scheduling (largest-first, wider channel buffer)
- Two-pass resize with CatmullRom for speed/quality balance
- Output file naming that avoids replacing the original source file
- User preset config persistence with install-dir-first, OS-config-dir fallback strategy
- Compression task registry with cancellation flag propagation
- System-handler integration for opening and revealing output files
- Splash window management (show on launch, close when app is ready)

## User Workflow

The app follows a two-stage backend workflow and a three-step UI flow.

1. Add one or more PDFs via drag-and-drop, browse, or native desktop file picker.
2. Let the app analyze each file and suggest a preset.
3. Review the selected file, adjust settings if needed, and export optimized copies.

The analyze-first rule is enforced in `src/composables/usePdfCompressor.ts`. Compression waits for each queued file to finish analysis before that file is handed to the backend compressor.

## Project Architecture

### High-level flow

```text
Vue UI -> Tauri bridge -> Rust commands -> PDF analysis/compression engine -> output PDF
```

### Frontend architecture

- `src/main.ts`
  - Vue entry point; loads global styles and mounts the app with i18n

- `src/App.vue`
  - Top-level application shell
  - Two-column layout: upload panel (primary) + sidebar (settings, activity)
  - Maps workflow state into user-facing status copy

- `src/composables/useTheme.ts`
  - Theme management composable (dark / light / system)
  - Persists preference in localStorage
  - Applies `data-theme` attribute to document root
  - Listens for OS theme changes

- `src/composables/usePdfCompressor.ts`
  - Source of truth for workflow state
  - Holds jobs, settings, analysis results, compression results, loading states, errors
  - Normalizes backend payloads into frontend types
  - Enforces analyze-first-then-compress
  - Manages concurrent compression with worker pool

- `src/lib/tauri.ts`
  - Thin bridge between Vue and Tauri commands
  - Detects whether native commands are available
  - Opens the desktop file picker and directory picker
  - Listens for native drag-and-drop events
  - Invokes `analyze_pdf`, `compress_pdf`, `cancel_compression`, and preset config commands
  - Provides window management (minimize, maximize, close, drag)

- `src/config/presets.ts`
  - Manages preset profiles (load, save, clear) with native persistence
  - Merges built-in defaults with user-saved overrides
  - Caches loaded config to avoid repeated disk reads

- `src/config/preset-defaults.json`
  - Built-in default values for each compression preset (quality, max image size percent)

- `src/utils/compressionSettings.ts`
  - Clamping and normalization for image quality, image size percent, and pixel values
  - Converts percentage-based max image size to absolute pixel values using a reference edge

- `src/utils/format.ts`
  - Formatting helpers for bytes, percentages, milliseconds, and file paths

- `src/i18n/index.ts`
  - Boots `vue-i18n`; supports `en` and `zh-CN`
  - Stores the selected locale in localStorage

### Native backend architecture

- `src-tauri/src/lib.rs` — Tauri application entry; registers plugins, manages splash window, and registers command handlers
- `src-tauri/src/commands.rs` — Tauri command surface; merges and normalizes settings from frontend input; preset config persistence; compression task registry with cancellation; open/reveal via system handler
- `src-tauri/src/models.rs` — Shared request/response structures serialized between Rust and Vue
- `src-tauri/src/error.rs` — Central backend error mapping with i18n-compatible error codes for user-facing failures
- `src-tauri/src/pdf/analyzer.rs` — Preflight analysis engine (page sampling, text density, image signals)
- `src-tauri/src/pdf/compressor.rs` — Object-level PDF optimization engine (image recompression, stream compression, metadata removal)
- `src-tauri/src/pdf/settings.rs` — Settings normalization with backend defaults and range clamping

## Directory Map

```text
.
├─ public/
│  └─ splash.html                   # Splash screen shown during app initialization
├─ src/
│  ├─ main.ts                       # Vue app entry
│  ├─ App.vue                       # Main shell: two-column layout
│  ├─ composables/
│  │  ├─ usePdfCompressor.ts        # Workflow state, command calls, payload normalization
│  │  └─ useTheme.ts                # Dark/light/system theme management
│  ├─ lib/
│  │  └─ tauri.ts                   # Native bridge, dialog, drag-and-drop, command invoke
│  ├─ config/
│  │  ├─ presets.ts                 # Preset profile management, persistence, default merging
│  │  └─ preset-defaults.json       # Built-in default values per preset
│  ├─ components/
│  │  ├─ PdfUploadPanel.vue         # Queue intake and drag-and-drop UI (primary view)
│  │  ├─ CompressionSettingsPanel.vue # Preset grid, sliders, toggles
│  │  ├─ ActivityPanel.vue          # Current job state, compress button, metrics
│  │  ├─ AppHeader.vue              # Brand, theme switcher, locale switcher, window controls
│  │  └─ ErrorToastViewport.vue     # Floating error/warning toast notifications
│  ├─ i18n/
│  │  └─ index.ts                   # Locale setup and persistence
│  ├─ locales/
│  │  ├─ en.ts
│  │  └─ zh-CN.ts
│  ├─ utils/
│  │  ├─ compressionSettings.ts     # Image quality/size clamping and pixel calculation
│  │  └─ format.ts                  # Formatting helpers (bytes, percent, path, ms)
│  └─ types/
│     └─ pdf.ts                     # Frontend PDF workflow types
├─ src-tauri/
│  ├─ Cargo.toml                    # Rust crate metadata and native dependencies
│  ├─ tauri.conf.json               # Tauri product and bundling config
│  └─ src/
│     ├─ lib.rs                     # Tauri builder entry, splash window setup
│     ├─ commands.rs                # Command surface, preset config, task registry
│     ├─ models.rs                  # Analysis and compression payloads
│     ├─ error.rs                   # Shared backend error type with i18n codes
│     └─ pdf/
│        ├─ analyzer.rs             # Preflight analysis engine
│        ├─ compressor.rs           # Compression engine
│        └─ settings.rs             # Settings normalization
├─ aur/
│  └─ pdf-compressor/                # Arch Linux (AUR) source package (PKGBUILD, .SRCINFO)
├─ package.json                     # Frontend scripts and JS dependencies
├─ README.md
└─ README.zh-CN.md
```

## Prerequisites

You need the usual toolchain for a Vue + Tauri desktop app:

- Node.js and npm
- Rust toolchain
- Tauri build prerequisites for your operating system

On Arch Linux the system dependencies are `webkit2gtk-4.1` and `gtk3` (plus `cargo`, `nodejs`, `npm`, and `pkgconf` to build); see `aur/pdf-compressor/PKGBUILD` for the authoritative list. Other distributions need the equivalent WebKit2GTK 4.1 and GTK 3 packages.

## Development

### Install dependencies

```bash
npm install
```

### Run the frontend only

```bash
npm run dev
```

This starts the Vite frontend only. It is useful for UI work, layout checks, and general frontend development.

Important limits in preview/browser mode:

- Native Tauri commands are not available
- The native file picker is disabled
- Drag-and-drop may not provide a usable desktop file path
- Real backend analysis and compression require the desktop shell

### Run the full desktop app during development

```bash
npm run tauri dev
```

This starts the Vue dev server and launches the Tauri shell. Use this mode when you want to verify native file browsing, drag-and-drop, backend analysis, compression output, and theme switching in the desktop environment.

### Tests

```bash
npm test                    # Frontend unit tests (Vitest)
cargo test --workspace      # Rust unit + pipeline integration tests
cargo bench -p pdf-core     # Compression benchmarks (criterion)
```

The Rust code is a Cargo workspace: `crates/pdf-core` is the pure PDF engine (analyzer, compressor, models, a `pdf-cli` binary, benchmarks, and the cargo-fuzz target), and `src-tauri` is the desktop shell. The Rust test suite includes an `export_bindings` test that regenerates `src/lib/bindings.ts` (the typed IPC layer produced by tauri-specta). Whenever a Tauri command signature changes, run `cargo test --workspace` and commit the regenerated bindings alongside the change.

A small CLI is available for shell use and debugging:

```bash
cargo run -p pdf-core --bin pdf-cli -- analyze <file.pdf>
cargo run -p pdf-core --bin pdf-cli -- compress <file.pdf> --preset maximum
cargo run -p pdf-core --bin pdf-cli -- compress <file.pdf> --target-size 5MB
```

The PDF engine also has a cargo-fuzz target (`crates/pdf-core/fuzz`) — run it from `crates/pdf-core` with `cargo +nightly fuzz run pipeline`.

### Versioning

`package.json` is the single source of truth for the app version. After bumping it, run:

```bash
npm run sync-version
```

This propagates the version to `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`.

## Build and Release

### Build the frontend bundle

```bash
npm run build
```

### Build desktop release artifacts

```bash
npm run tauri build
```

This produces the NSIS installer. To also output a portable (no-install) executable alongside the installer:

```bash
npm run tauri:build
```

The portable copy is placed at `src-tauri/target/release/bundle/PDF-Compressor-portable.exe`.

The portable executable requires Windows 10 21H2+ or Windows 11 (WebView2 is pre-installed on these systems).

Release metadata:

- Product name: `PDF Compressor`
- Version: `0.2.0`
- Author: `cosct`
- Identifier: `com.cosct.pdfcompressor`

### Linux packaging (Arch / AUR)

For Arch-based distributions, packaging is provided via the AUR source package in `aur/pdf-compressor/`. The `PKGBUILD` runs `npm ci && npm run build`, builds the release binary with `cargo build --release --locked`, and installs it as `/usr/bin/pdf-compressor` along with a desktop entry and icons. See [Installation](#installation) for usage and `aur/pdf-compressor/README.md` for the publishing checklist.

## Runtime Details

### Analysis pass

The analysis step in `src-tauri/src/pdf/analyzer.rs` is a lightweight preflight pass. It evaluates:

- File size
- Exact page count from the PDF page tree
- Embedded image signals from page resources and XObjects
- Extracted text density across sampled pages
- Fallback structural text signals from page content operators and font resources
- Scanned-document confidence and estimated image coverage

The backend returns `documentKind`, `recommendedPreset`, `estimatedSavingsPercent`, and notices. This is heuristic-based guidance, not an exact guarantee.

### Compression pass

The compressor in `src-tauri/src/pdf/compressor.rs` works at the PDF object level:

- Image streams are inspected and recompressed only when safe
- Supported images may be resized using a two-pass strategy (Nearest + CatmullRom)
- Recompressed images are encoded as JPEG
- Eligible non-image streams may be deflate-compressed
- Metadata can be removed from document info and root metadata entries
- The compressor explicitly preserves text and vector instructions

### Theme system

The theme system uses CSS custom properties with two complete token sets (dark and light). Theme switching is instant with no page reload. The `data-theme` attribute on `<html>` controls which token set is active. On first launch, the theme defaults to following the OS preference (`system` mode). A synchronous script in `index.html` prevents flash-of-wrong-theme on startup, with a CSS `prefers-color-scheme` media query as an additional fallback.

## Output Behavior

The app writes a new file beside the original source PDF. It does not overwrite the input file.

Naming format: `<original-name>__optimized-<preset>.pdf`

If that name exists, the backend appends a numeric suffix (`-1`, `-2`, etc.) until it finds a free name.

## Localization

Current locales: `en` and `zh-CN`

Behavior:
- Reads from localStorage first
- Falls back to browser language detection
- Chinese browser locales default to `zh-CN`
- Everything else falls back to `en`

## Troubleshooting

### The browse button is disabled

Expected in browser preview mode. Run `npm run tauri dev` for native browse.

### Compression finished but the file did not get smaller

Possible causes: text-native PDFs, already-optimized files, few/no embedded images, or unsupported image filters. Try a stronger preset or lower image quality.

### Some images were skipped

Expected for certain image objects. The backend skips streams it cannot safely rewrite (transparency, masks, JPX/JBIG2/CCITT/Crypt filters, unsupported color spaces).

### Large PDFs feel slow

The analyzer warns when page count is high. Large or image-heavy PDFs require inspecting and rewriting many objects. The compressor uses parallel workers when multiple images are present.

## Limitations

- Compression quality is heuristic-based; estimated savings are guidance, not guarantees
- Analysis is pure-Rust and structure-based — no page rendering
- Some embedded image formats and protected structures are intentionally skipped
- Only safe object-level optimizations are attempted
- Frontend depends on the desktop shell for native commands
- `compress_scanned_pdf` exists in the backend but is not used by the current Vue workflow

## Tech Stack

- Vue 3 + TypeScript + Vite
- Tauri 2 + Rust
- `lopdf` (PDF parsing and writing)
- `image` (image decode, resize, JPEG encode)
- `vue-i18n` (internationalization)

## Release History

### v0.2.0 (2026-04-16)

- Fluent Design UI overhaul with dark/light/system theme support
- Theme defaults to system preference on first launch (no hardcoded dark fallback)
- PDF upload panel promoted to primary view with improved layout
- Acrylic header with backdrop-filter effects
- Custom window chrome with integrated titlebar controls
- Splash screen during initial app load
- Compression speed optimization: CatmullRom resize, earlier two-pass threshold, wider channel buffer, lower parallel threshold
- User preset persistence: per-preset overrides saved to disk with install-dir-first strategy
- Compression cancellation with per-task cancel flag propagation
- Output directory selection for compressed files
- Open and reveal compressed output files via system handler
- Error toast viewport for surfacing backend and dialog errors
- Percentage-based max image size slider with reference edge from analysis
- Toggle switches for boolean settings
- Inline SVG panel icons
- Production build outputs portable executable alongside NSIS installer
- Updated README with detailed architecture and change documentation

### v0.1.0 (2026-03-27)

- Initial desktop workflow: Vue + Tauri app shell, native PDF intake, preflight analysis, object-level compression, English and Chinese UI
