const zhCN = {
  locale: {
    label: '语言',
    en: 'English',
    'zh-CN': '简体中文',
  },
  theme: {
    label: '主题',
    system: '跟随系统',
    light: '浅色',
    dark: '深色',
  },
  app: {
    alertTitle: '提示',
    emptySource: '未选择',
    preset: {
      conservative: '保守',
      balanced: '均衡',
      maximum: '最大化',
      custom: '自定义',
    },
  },
  header: {
    title: 'PDF 压缩器',
    minimize: '最小化',
    maximize: '最大化',
    restore: '还原',
    close: '关闭',
  },
  intake: {
    browse: '选择文件',
    browseDisabledPreview: '选择文件（仅桌面版）',
    invalidDrop: '仅支持 .pdf 文件',
  },
  upload: {
    eyebrow: 'PDF 上传',
    dropTitle: '将 PDF 文件拖到这里',
    dropHint: '或点击下方按钮选择文件',
    dropHintReady: '继续拖入文件或点击按钮追加到队列',
    queueHint: '整个区域都支持继续拖入 PDF',
    queueTitle: '文件队列',
    lockedTag: '压缩中',
    lockedHint: '压缩进行中，队列已锁定',
  },
  settings: {
    eyebrow: '压缩参数预设',
    recommendedBadge: '推荐',
    presetGroupLabel: '压缩预设',
    advancedToggle: '高级设置',
    quality: '图片质量',
    maxEdge: '最大边长',
    optimizeImages: '优化图片',
    compressStreams: '压缩流',
    stripMetadata: '移除元数据',
    outputDir: '输出目录',
    outputDirDefault: '与原文件相同',
    outputDirBrowse: '选择',
    targetSize: '目标大小（MB）',
    targetSizeHint: '例如 5 — 自动压缩到该体积以内',
    targetSizeOff: '关闭',
    savePreset: '保存预设',
    resetPresets: '恢复默认',
    applyToAll: '应用到全部',
  },
  queue: {
    count: '{count} 项',
    overrideTag: '已覆盖推荐参数',
    delete: '删除',
    contextMenuLabel: '队列项操作',
    openCompressedFile: '打开压缩后 PDF',
    openCompressedFolder: '打开压缩文件所在文件夹',
    pages: '页',
    status: {
      selected: '等待',
      analyzing: '分析中',
      ready: '就绪',
      compressing: '压缩中',
      success: '完成',
      error: '错误',
    },
    detail: {
      selected: '文件已加入队列',
      analyzing: '正在分析文档结构',
      ready: '可以开始压缩',
      compressing: '正在生成输出文件',
      success: '压缩完成',
      error: '需要处理',
    },
  },
  activity: {
    eyebrow: '活动',
    liveLabel: '状态',
    metricSavings: '节省',
    metricProgress: '进度',
    metricQueued: '队列',
    metricCompleted: '完成',
    startCompression: '开始压缩',
    noSourcePlaceholder: '没有添加文件',
    cancel: '取消压缩',
    outputLocked: '压缩进行中，输出目录已锁定。',
    states: {
      idleTitle: '等待文件',
      selectedTitle: '文件已就绪',
      analyzingTitle: '分析中',
      readyTitle: '可以压缩',
      compressingTitle: '压缩中',
      successTitle: '完成',
      errorTitle: '错误',
    },
  },
  composable: {
    notices: {
      checkTitle: '需要关注',
      analysisNoteTitle: '分析提示',
      backendNoteTitle: '处理提示',
      followUpTitle: '后续提示',
    },
    errors: {
      backendFallback: '桌面服务返回了未知错误。',
      browseRequiresDesktop: '文件浏览仅在 Tauri 桌面应用中可用。',
      queueLocked: '压缩任务进行期间无法添加文件。',
    },
  },
  backend: {
    'analysis.note.structureBased': {
      body: '分析基于 PDF 结构检查，而非页面渲染。',
    },
    'analysis.note.safeOptimization': {
      body: '压缩针对图片流、元数据和可压缩的 PDF 流。',
    },
    'analysis.note.sampledPages': {
      body: '检测到大体积 PDF，详细检查抽样了 {pageCount} 页中的 {inspectedPages} 页。',
    },
    'analysis.warning.textExtractionFallback': {
      body: '部分页面无法完全解码，因此推荐保持保守。',
    },
    'analysis.warning.textNative': {
      body: '该文件以原生文本为主，压缩空间可能有限。',
    },
    'analysis.warning.largePageCount': {
      body: '该 PDF 页数较多，准备和压缩可能需要更长时间。',
    },
    'analysis.warning.noImages': {
      body: '未检测到内嵌图片对象，压缩收益可能依赖流压缩和元数据清理。',
    },
    'analysis.note.mixedDocument': {
      body: '该 PDF 混合了可读文本结构与图片密集页面，推荐先采用更稳妥的设置。',
    },
    'analysis.note.smallPdf': {
      body: '小体积 PDF 通常难以大幅缩减。',
    },
    'compress.note.appliedProfile': {
      body: '已应用“{preset}”预设：JPEG 质量 {quality}，图片最大边长 {maxImageSizePx} px。',
    },
    'compress.note.safeRewrite': {
      body: '当重写不安全时，优化器会保留文本与矢量指令。',
    },
    'compress.note.imageSkipSummary': {
      body: '为保持报告简洁，已合并 {count} 条额外的图片跳过提示。',
    },
    'compress.note.metadataKept': {
      body: '因元数据清理已禁用或不可用，文档元数据保持原样。',
    },
    'compress.note.imageDedupe': {
      body: '已将 {count} 个重复图片对象合并为共享引用。',
    },
    'compress.note.targetAttempt': {
      body: '正在逼近目标大小：尝试 JPEG 质量 {quality}（第 {attempt} 次）。',
    },
    'compress.note.targetSizeMet': {
      body: '已以 JPEG 质量 {quality} 达成 {targetKb} KB 的目标大小。',
    },
    'compress.warning.targetSizeMissed': {
      body: '无法达到 {targetKb} KB 的目标；已生成当前可达的最小结果。',
    },
    'compress.warning.imageSkipped': {
      body: '已跳过图片对象 {objectId}：{reason}',
    },
  },
  error: {
    'error.missingInput': {
      body: '所选文件不存在：{path}',
    },
    'error.invalidPdfPath': {
      body: '所选文件不是 PDF：{path}',
    },
    'error.image': {
      body: '图片处理失败：{detail}',
    },
    'error.io': {
      body: '文件系统操作失败：{detail}',
    },
    'error.cancelled': {
      title: '已取消',
      body: '当前压缩任务已取消。',
    },
    'error.config': {
      body: '配置操作失败：{detail}',
    },
    'error.opener': {
      body: '调用系统程序打开路径失败：{detail}',
    },
    'error.pdfBuild': {
      body: '生成输出 PDF 失败：{detail}',
    },
  },
}

export default zhCN
