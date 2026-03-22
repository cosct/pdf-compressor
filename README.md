# PDF Compressor

Language versions: `README.md` (English) | `README.zh-CN.md` (简体中文)

PDF Compressor is a local-first desktop app for reducing PDF size with a Rust backend and a Vue 3 + Tauri frontend. It is built around a simple single-file workflow: choose one PDF, analyze whether compression is worth it, adjust a focused set of settings, and export a lighter copy without overwriting the original.

Current release: `0.1.0`

Author: `cosct`

## Overview

This project is designed for selective PDF optimization rather than blind whole-file rewriting. The current pipeline tries to preserve text and vector instructions whenever possible, then reduces size by working on areas that are usually safer to optimize:

- embedded image streams can be recompressed as JPEG and resized when needed
- eligible non-image PDF streams can be compressed
- document metadata can be removed
- the UI runs an analysis pass first so the user can review likely savings and a suggested preset before export

The application is desktop-first and local-first. There is no upload flow, no cloud processing, and no batch queue in this release.

## Main Features

### User-facing features

- Single PDF intake with drag and drop, manual path entry, and native desktop browse
- Preflight analysis before compression
- Three presets: `conservative`, `balanced`, `maximum`
- Adjustable image quality and maximum image edge
- Toggles for image optimization, stream compression, and metadata removal
- Result view with output path, elapsed time, size delta, and optimization counts
- English and Simplified Chinese UI

### Backend features

- PDF structure inspection through `lopdf`
- Pure-Rust preflight analysis based on page maps, page resources, and extractable text
- Heuristic document classification: `text-native`, `mixed`, or `scan-heavy`
- Suggested preset based on scanned-document confidence
- Safe-skip behavior for image streams that are not yet supported for rewriting
- Output file naming that avoids replacing the original source file

## User Workflow

The app follows a two-stage backend workflow and a three-step UI flow.

1. Add one PDF.
2. Run analysis to estimate whether compression is worth it.
3. Review the recommendation, adjust settings, and export a new optimized copy.

The analyze-first rule is enforced in `src/composables/usePdfCompressor.ts`. Compression remains disabled until analysis has completed for the current source path.

## Project Architecture

### High-level flow

```text
Vue UI -> Tauri bridge -> Rust commands -> PDF analysis/compression engine -> output PDF
```

### Frontend architecture

- `src/main.ts`
  - Vue entry point
  - loads global styles and mounts the app with i18n

- `src/App.vue`
  - top-level application shell
  - composes the intake, analysis, settings, and result panels
  - maps workflow state into user-facing status copy

- `src/composables/usePdfCompressor.ts`
  - source of truth for workflow state
  - holds the selected path, settings, analysis result, compression result, loading states, and errors
  - normalizes backend payloads into frontend types
  - enforces analyze first, then compress

- `src/lib/tauri.ts`
  - thin bridge between Vue and Tauri commands
  - detects whether native commands are available
  - opens the desktop file picker
  - listens for native drag-and-drop events
  - invokes `analyze_pdf` and `compress_pdf`

- `src/i18n/index.ts`
  - boots `vue-i18n`
  - supports `en` and `zh-CN`
  - stores the selected locale in local storage

### Native backend architecture

- `src-tauri/src/lib.rs`
  - Tauri application entry
  - registers plugins and command handlers

- `src-tauri/src/commands.rs`
  - exposes Tauri commands to the frontend
  - merges and normalizes compression settings from frontend input
  - currently registers `analyze_pdf`, `compress_pdf`, and `compress_scanned_pdf`

- `src-tauri/src/models.rs`
  - shared request and response structures serialized between Rust and Vue

- `src-tauri/src/error.rs`
  - central backend error mapping for user-facing failures

- `src-tauri/src/pdf/analyzer.rs`
  - performs the preflight analysis pass
  - inspects file size, exact page count, extractable text density, and image/XObject structure
  - estimates scanned confidence, image coverage, likely savings, and a recommended preset

- `src-tauri/src/pdf/compressor.rs`
  - performs object-level PDF optimization
  - recompresses supported image streams
  - compresses eligible non-image streams
  - optionally removes document metadata
  - writes an output file with a generated name

- `src-tauri/src/pdf/settings.rs`
  - normalizes presets and settings
  - applies backend defaults and clamps accepted ranges

## Directory Map

```text
.
├─ src/
│  ├─ main.ts                       # Vue app entry
│  ├─ App.vue                       # Main shell and panel composition
│  ├─ composables/
│  │  └─ usePdfCompressor.ts        # Workflow state, command calls, payload normalization
│  ├─ lib/
│  │  └─ tauri.ts                   # Native bridge, dialog, drag-and-drop, command invoke
│  ├─ components/
│  │  ├─ FileIntakePanel.vue        # Source path entry and drag-and-drop UI
│  │  ├─ AnalysisPanel.vue          # Analysis summary
│  │  ├─ CompressionSettingsPanel.vue
│  │  ├─ ResultPanel.vue
│  │  └─ AppHeader.vue
│  ├─ i18n/
│  │  └─ index.ts                   # Locale setup and persistence
│  ├─ locales/
│  │  ├─ en.ts
│  │  └─ zh-CN.ts
│  └─ types/
│     └─ pdf.ts                     # Frontend PDF workflow types
├─ src-tauri/
│  ├─ Cargo.toml                    # Rust crate metadata and native dependencies
│  ├─ tauri.conf.json               # Tauri product and bundling config
│  └─ src/
│     ├─ lib.rs                     # Tauri builder entry
│     ├─ commands.rs                # Command surface
│     ├─ models.rs                  # Analysis and compression payloads
│     ├─ error.rs                   # Shared backend error type
│     └─ pdf/
│        ├─ analyzer.rs             # Preflight analysis engine
│        ├─ compressor.rs           # Compression engine
│        └─ settings.rs             # Settings normalization
├─ package.json                     # Frontend scripts and JS dependencies
└─ README.md
```

## Prerequisites

You need the usual toolchain for a Vue + Tauri desktop app:

- Node.js and npm
- Rust toolchain
- Tauri build prerequisites for your operating system

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

- native Tauri commands are not available
- the native file picker is disabled
- drag and drop may not provide a usable desktop file path the same way the desktop shell does
- manual path entry remains available in the UI
- real backend analysis and compression require the desktop shell

That means preview mode supports the manual path flow at the interface level, but native browse requires the desktop shell.

### Run the full desktop app during development

```bash
npm run tauri dev
```

This is the main end-to-end development path. It starts the Vue dev server and launches the Tauri shell defined in `src-tauri/tauri.conf.json`.

Use this mode when you want to verify:

- native file browsing
- native drag and drop
- backend analysis
- compression output generation
- pure-Rust PDF inspection and optimization

## Build and Release

### Build the frontend bundle

```bash
npm run build
```

This runs:

- `vue-tsc -b`
- `vite build`

### Build desktop release artifacts

```bash
npm run tauri build
```

Release metadata is defined in:

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Current first release metadata:

- product name: `PDF Compressor`
- version: `0.1.0`
- author: `cosct`
- identifier: `com.cosct.pdfcompressor`

The Tauri bundle target is currently set to `all` in `src-tauri/tauri.conf.json`.

## Runtime Details

### Analysis pass

The analysis step in `src-tauri/src/pdf/analyzer.rs` is a lightweight preflight pass that helps the UI answer a practical question: is this file likely to shrink enough to be worth exporting?

It currently evaluates:

- file size
- exact page count from the PDF page tree
- embedded image signals from page resources and XObjects when available
- extracted text density across pages when `lopdf` can decode it
- fallback structural text signals from page content operators and font resources
- scanned-document confidence
- estimated image coverage

From that, the backend returns:

- `documentKind`
- `recommendedPreset`
- `estimatedSavingsPercent`
- warnings and notes for the UI

This is heuristic-based guidance, not an exact guarantee of compression output.

Because this pass is now pure Rust and renderer-free, it is intentionally conservative when a PDF has unusual encodings, partial OCR layers, or complex resource graphs. Ambiguous documents are more likely to land in `mixed` than to be overstated as `scan-heavy`.

### Compression pass

The compressor in `src-tauri/src/pdf/compressor.rs` works at the PDF object level.

Current behavior:

- image streams are inspected and recompressed only when the stream looks safe to rewrite
- supported image content may be resized down to the configured maximum edge
- recompressed images are encoded as JPEG
- eligible non-image streams may be compressed if not already compressed
- metadata can be removed from document info and root metadata entries

The compressor explicitly tries to preserve text and vector instructions whenever it cannot safely rewrite an object.

## Using the App

### Step 1: Add one PDF

From the intake panel you can:

- drag and drop a PDF into the window
- use the native desktop browse button when running inside the Tauri shell
- paste a full path to a `.pdf` file manually

If the current path does not end with `.pdf`, analysis and compression remain disabled in the frontend.

### Step 2: Analyze the file

Click `Analyze PDF` to run the native analysis pass.

The analysis view shows:

- source size
- page count
- embedded image count
- detected document fit
- estimated savings
- recommended preset
- warnings and notes

The recommended preset is surfaced in the settings panel.

### Step 3: Adjust settings and compress

The settings panel exposes:

- preset
- image quality
- max image edge
- optimize images toggle
- compress streams toggle
- remove metadata toggle

Once analysis is complete, click `Compress PDF` to generate the optimized copy.

### Workflow states

The frontend models these states in `src/types/pdf.ts` and `src/composables/usePdfCompressor.ts`:

- `idle`
- `selected`
- `analyzing`
- `ready`
- `compressing`
- `success`
- `error`

## Output Behavior

The app writes a new file beside the original source PDF. It does not overwrite the input file.

Current naming format:

```text
<original-name>__optimized-<preset>.pdf
```

Example:

```text
report.pdf
report__optimized-balanced.pdf
```

If that name already exists, the backend appends a numeric suffix such as `-1`, `-2`, and so on until it finds a free name.

The result panel reports:

- output path
- elapsed time
- original size
- compressed size
- bytes saved
- percent saved
- images optimized
- images skipped
- streams packed

If the output is not smaller than the source, the UI warns about that explicitly so you can inspect the result before deciding what to keep.

## Localization

Localization is initialized in `src/i18n/index.ts`.

Current locales:

- `en`
- `zh-CN`

Behavior:

- locale is read from local storage first
- otherwise the app checks the browser language
- Chinese browser locales default to `zh-CN`
- everything else falls back to `en`
- the selected locale is stored in local storage under `pdf-compressor-locale`

Translation content currently lives in:

- `src/locales/en.ts`
- `src/locales/zh-CN.ts`

## Troubleshooting

### The browse button is disabled

That is expected in browser preview mode. Native browse is only available inside the Tauri desktop shell.

Use one of these instead:

- run `npm run tauri dev`
- paste a full `.pdf` path manually into the input field

### Analysis or compression says the path is invalid

Check that:

- the file exists
- the path ends with `.pdf`
- the app can access the file from the current desktop session

The backend validates both existence and the `.pdf` extension before processing.

### Compression finished but the file did not get smaller

That can happen with:

- text-native PDFs
- already-optimized PDFs
- files with few or no embedded images
- image filters the compressor intentionally skips

Try a stronger preset or lower image quality only if additional loss is acceptable.

### Some images were skipped

This is expected for some image objects. The current backend skips streams it does not yet safely rewrite, including cases such as:

- transparency or image masks
- unsupported filters like `JPXDecode`, `JBIG2Decode`, `CCITTFaxDecode`, or `Crypt`
- unsupported raw image layouts
- image data that fails safe decoding

### Large PDFs feel slow

The analyzer already warns when page count is high. Large or image-heavy PDFs take longer because the backend must inspect and sometimes rewrite many objects.

## Limitations

This release is intentionally focused and has several clear limits.

- The app is built around one PDF at a time. There is no batch workflow.
- Compression quality is heuristic-based. Estimated savings are guidance, not a guarantee.
- The analysis pass is pure-Rust and structure-based. It does not render pages, so text extraction and scan detection are approximate for some PDFs.
- Some embedded image formats and protected or complex structures are skipped on purpose to avoid damaging the document.
- Only safe object-level optimizations are attempted. The app does not promise aggressive rewriting of every PDF structure.
- The frontend depends on the desktop shell for native commands. Browser preview mode is useful for UI work, not full PDF processing.
- A `compress_scanned_pdf` command exists in the backend command surface, but the current Vue workflow uses `analyze_pdf` and `compress_pdf`.

## Tech Stack

- Vue 3
- TypeScript
- Vite
- Tauri 2
- Rust
- `lopdf`
- `image`
- `printpdf`

## Release Notes for v0.1.0

Version `0.1.0` establishes the first complete desktop workflow for this project:

- Vue + Tauri app shell
- native PDF path intake
- preflight PDF analysis
- object-level compression pipeline
- localized English and Simplified Chinese UI
- packaged desktop metadata for `PDF Compressor`
