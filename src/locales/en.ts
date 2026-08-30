const en = {
  locale: {
    label: 'Language',
    en: 'English',
    'zh-CN': '简体中文',
  },
  theme: {
    label: 'Theme',
    system: 'System',
    light: 'Light',
    dark: 'Dark',
  },
  app: {
    alertTitle: 'Notice',
    emptySource: 'Not selected',
    preset: {
      conservative: 'Conservative',
      balanced: 'Balanced',
      maximum: 'Maximum',
      custom: 'Custom',
    },
  },
  header: {
    title: 'PDF Compressor',
    minimize: 'Minimize',
    maximize: 'Maximize',
    restore: 'Restore',
    close: 'Close',
    settings: 'Settings',
    back: 'Back',
  },
  settingsView: {
    title: 'Settings',
  },
  appearance: {
    title: 'Appearance',
  },
  quick: {
    title: 'Right-click quick compress',
    params: 'Compression parameters',
    followsPresets:
      'Parameters follow the compression preset profiles (read-only); the color mode can be adjusted here, and target-size mode fits the budget with the best achievable quality.',
    save: 'Save quick settings',
    saving: 'Saving…',
    savedTitle: 'Quick settings saved',
    savedBody: 'Right-click compression now uses these settings.',
    note: 'Applies to right-click actions on this machine.',
  },
  intake: {
    browse: 'Browse',
    browseDisabledPreview: 'Browse (desktop only)',
    invalidDrop: 'Only .pdf files allowed',
  },
  upload: {
    eyebrow: 'PDF Upload',
    dropTitle: 'Drop PDF files here',
    dropHint: 'or click the button below to select files',
    dropHintReady: 'Drop more files or use the button to append to the queue',
    queueHint: 'You can keep dropping PDFs anywhere in this area',
    queueTitle: 'File queue',
    addMore: 'Add files',
    lockedTag: 'Compressing',
    lockedHint: 'Queue is locked while compression is running',
  },
  settings: {
    eyebrow: 'Compression Preset',
    compressionMode: 'Compression mode',
    recommendedBadge: 'Recommended',
    recommendedShort: 'Rec',
    presetGroupLabel: 'Compression presets',
    advancedToggle: 'Advanced settings',
    quality: 'Image quality',
    qualityHint: 'JPEG re-encode quality (1–100); lower means smaller files',
    maxEdge: 'Image size cap',
    maxEdgeHint: 'Images with a longer edge are downscaled; the value is a percentage of the document\'s largest edge',
    optimizeImages: 'Optimize images',
    optimizeImagesHint: 'Re-encode document images to shrink them; off keeps images untouched',
    compressStreams: 'Compress streams',
    compressStreamsHint: 'Apply lossless (Flate) compression to content streams and other compressible objects',
    stripMetadata: 'Remove metadata',
    stripMetadataHint: 'Strip author/producer and other document information from the metadata',
    colorMode: 'Color mode',
    colorModeColor: 'Color',
    colorModeGray: 'Grayscale',
    colorModeBw: 'Black & white (G4)',
    subsetFonts: 'Subset fonts',
    subsetFontsHint: 'Trim embedded fonts to the glyphs the document actually uses',
    outputDir: 'Output directory',
    outputDirDefault: 'Same as source',
    outputDirBrowse: 'Browse',
    targetMode: 'Target size',
    targetSizeUnit: 'Unit',
    targetSizeOff: 'Off',
    targetSizeInvalid: 'Enter a size between 0.1 and 2048 MB.',
    savePreset: 'Save preset',
    resetPresets: 'Reset defaults',
    applyToAll: 'Apply to all',
    applyToAllHintSingle: 'Add at least two files to apply settings to all.',
    presetSavedTitle: 'Preset saved',
    presetSavedBody: 'The current quality and max-edge settings were saved to this preset.',
  },
  queue: {
    count: '{count} item | {count} items',
    overrideTag: 'Custom settings',
    delete: 'Delete',
    contextMenuLabel: 'Queue item actions',
    openCompressedFile: 'Open compressed PDF',
    openCompressedFolder: 'Open containing folder',
    pageCount: '{count} page | {count} pages',
    status: {
      selected: 'Waiting',
      analyzing: 'Analyzing',
      ready: 'Ready',
      compressing: 'Compressing',
      success: 'Done',
      error: 'Error',
    },
    detail: {
      selected: 'Added to queue',
      analyzing: 'Analyzing structure',
      ready: 'Ready to compress',
      compressing: 'Creating copy',
      success: 'Compression done',
      error: 'Needs attention',
    },
  },
  activity: {
    eyebrow: 'Activity',
    liveLabel: 'Status',
    metricSavings: 'Saved',
    metricProgress: 'Progress',
    metricQueued: 'Queued',
    metricCompleted: 'Done',
    startCompression: 'Start compression',
    startCompressionAll: 'Compress all ({count})',
    compressSelected: 'Compress selected only',
    recompress: 'Re-compress',
    cancel: 'Cancel compression',
    outputLocked: 'Output directory is locked while compression is running.',
    outputDirDesktopOnly: 'Choosing an output directory is only available inside the Tauri app.',
    report: {
      savedValue: '{size} saved',
      elapsedValue: 'took {time}',
      imagesValue: '{recompressed} images recompressed, {skipped} skipped',
      dedupValue: '{count} duplicates merged',
      streamsValue: '{count} streams compressed',
    },
    states: {
      idleTitle: 'Waiting for file',
      selectedTitle: 'File ready',
      analyzingTitle: 'Analyzing',
      readyTitle: 'Ready to compress',
      compressingTitle: 'Compressing',
      successTitle: 'Done',
      errorTitle: 'Error',
    },
  },
  composable: {
    notices: {
      backendNoteTitle: 'Processing note',
      restoreSkippedTitle: 'Queue restored',
      restoreSkippedBody: '{count} file(s) from the last session could not be found and were skipped.',
      scanPipelineTitle: 'Scanned-document mode',
      scanPipelineBody: 'This file looks scan-heavy, so the scanned-document pipeline (forced image and stream optimization) was used.',
    },
    errors: {
      backendFallback: 'The desktop service returned an unknown error.',
      browseRequiresDesktop: 'Desktop file browsing is only available inside the Tauri app.',
      queueLocked: 'Files cannot be added while a compression run is in progress.',
    },
  },
  backend: {
    'analysis.note.structureBased': {
      body: 'Analysis uses PDF structure inspection instead of page rendering.',
    },
    'analysis.note.safeOptimization': {
      body: 'Compression focuses on image streams, metadata, and compressible PDF streams.',
    },
    'analysis.note.sampledPages': {
      body: 'Large PDF detected, so detailed page inspection sampled {inspectedPages} of {pageCount} pages.',
    },
    'analysis.warning.textExtractionFallback': {
      body: 'Some pages could not be fully decoded, so the recommendation stays conservative.',
    },
    'analysis.warning.textNative': {
      body: 'This file looks mostly text-native, so savings may stay modest.',
    },
    'analysis.warning.largePageCount': {
      body: 'This PDF has many pages, so preparation and compression may take longer.',
    },
    'analysis.warning.noImages': {
      body: 'No embedded image objects were detected, so savings may rely on stream compression and metadata cleanup.',
    },
    'analysis.note.mixedDocument': {
      body: 'This PDF mixes readable text structure with image-heavy pages, so the recommendation favors a safer first pass.',
    },
    'analysis.note.smallPdf': {
      body: 'Small PDFs often have less room to shrink dramatically.',
    },
    'analysis.warning.unsupportedImageCodecs': {
      body: '{count} images use codecs (JBIG2, JPX, CCITT) that this version cannot re-encode; they are preserved as-is and excluded from the estimate.',
    },
    'compress.note.streamDedupe': {
      body: 'Merged {count} duplicate non-image stream(s) (content, fonts, forms) into shared references.',
    },
    'compress.note.resourcesCleaned': {
      body: 'Removed {count} unused font/XObject resource entries left behind by earlier edits.',
    },
    'compress.note.fontsSubsetted': {
      body: 'Subset {count} embedded font(s) to their used glyphs, shedding {savedKb} KB of font data.',
    },
    'analysis.note.encryptedUnlocked': {
      body: 'This PDF used owner-password encryption and was unlocked with the empty user password; the compressed export will be unencrypted.',
    },
    'compress.note.appliedProfile': {
      body: "Applied the '{preset}' profile with JPEG quality {quality} and max image edge {maxImageSizePx} px.",
    },
    'compress.note.safeRewrite': {
      body: 'The optimizer preserves text and vector instructions when a rewrite is not safe.',
    },
    'compress.note.imageSkipSummary': {
      body: 'Suppressed {count} additional image skip notices to keep the report concise.',
    },
    'compress.note.metadataKept': {
      body: 'Document metadata stayed in place because metadata cleanup was disabled or unavailable.',
    },
    'compress.note.imageDedupe': {
      body: 'Merged {count} duplicate image objects into shared references.',
    },
    'compress.note.targetAttempt': {
      body: 'Fitting to the target size: trying JPEG quality {quality} (attempt {attempt}).',
    },
    'compress.note.targetSizeMet': {
      body: 'Met the {targetKb} KB target size with JPEG quality {quality}.',
    },
    'compress.warning.targetSizeMissed': {
      body: 'Could not reach the {targetKb} KB target; produced the best achievable result instead.',
    },
    'compress.note.decryptedInput': {
      body: 'The input used owner-password encryption and was read with the empty user password; the output is written unencrypted.',
    },
    'compress.warning.outputNotSmaller': {
      body: 'Optimization could not beat the original {originalBytes} bytes (best result: {bestBytes} bytes); nothing was written.',
    },
    'compress.warning.imageSkipped': {
      body: 'Skipped image object {objectId}: {reason}',
    },
  },
  error: {
    'error.missingInput': {
      body: 'The selected file does not exist: {path}',
    },
    'error.invalidPdfPath': {
      body: 'The selected file is not a PDF: {path}',
    },
    'error.encryptedPdf': {
      body: 'This PDF is password-protected or DRM-encrypted; encrypted documents are not supported.',
    },
    'error.image': {
      body: 'Image processing failed: {detail}',
    },
    'error.io': {
      body: 'Filesystem operation failed: {detail}',
    },
    'error.cancelled': {
      title: 'Cancelled',
      body: 'The active compression task was cancelled.',
    },
    'error.config': {
      body: 'Configuration operation failed: {detail}',
    },
    'error.opener': {
      body: 'Failed to open the path with the system handler: {detail}',
    },
    'error.pdfBuild': {
      body: 'Failed to build the output PDF: {detail}',
    },
  },
}

export default en
