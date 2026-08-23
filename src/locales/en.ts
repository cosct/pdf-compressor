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
    lockedTag: 'Compressing',
    lockedHint: 'Queue is locked while compression is running',
  },
  settings: {
    eyebrow: 'Compression Preset',
    recommendedBadge: 'Recommended',
    presetGroupLabel: 'Compression presets',
    advancedToggle: 'Advanced settings',
    quality: 'Image quality',
    maxEdge: 'Max edge',
    optimizeImages: 'Optimize images',
    compressStreams: 'Compress streams',
    stripMetadata: 'Remove metadata',
    outputDir: 'Output directory',
    outputDirDefault: 'Same as source',
    outputDirBrowse: 'Browse',
    targetSize: 'Target size (MB)',
    targetSizeHint: 'e.g. 5 — auto-fit under a size',
    targetSizeOff: 'Off',
    savePreset: 'Save preset',
    resetPresets: 'Reset defaults',
    applyToAll: 'Apply to all',
  },
  queue: {
    count: '{count} items',
    overrideTag: 'Custom settings',
    delete: 'Delete',
    contextMenuLabel: 'Queue item actions',
    openCompressedFile: 'Open compressed PDF',
    openCompressedFolder: 'Open containing folder',
    pages: 'pages',
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
    noSourcePlaceholder: 'No file added',
    cancel: 'Cancel compression',
    outputLocked: 'Output directory is locked while compression is running.',
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
      checkTitle: 'Needs review',
      analysisNoteTitle: 'Analysis note',
      backendNoteTitle: 'Processing note',
      followUpTitle: 'Follow-up',
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
