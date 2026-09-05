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
    settings: '设置',
    back: '返回',
  },
  settingsView: {
    title: '设置',
  },
  appearance: {
    title: '外观',
  },
  quick: {
    title: '右键快速压缩',
    params: '压缩参数',
    followsPresets:
      '压缩参数跟随设置中“压缩参数预设”的配置（只读）；色彩模式可在此调整，目标大小模式下会在预算内自动追求最佳质量。',
    save: '保存快速设置',
    saving: '正在保存…',
    savedTitle: '快速设置已保存',
    savedBody: '右键压缩现在使用这组参数。',
    note: '仅作用于本机的右键压缩动作。',
  },
  password: {
    title: '需要密码',
    requiredHint: '该 PDF 设置了打开密码，输入密码后继续。',
    wrongPasswordHint: '刚才的密码未能解锁该文件，请重试。',
    placeholder: '打开密码',
    submit: '解锁',
    cancel: '取消',
  },
  updater: {
    title: '软件更新',
    checkForUpdates: '检查更新',
    checking: '正在检查…',
    upToDateTitle: '已是最新版本',
    upToDateBody: '当前已运行最新版本（{version}）。',
    availableTitle: '发现新版本',
    availableBody: '新版本 {version} 可用，是否立即下载并安装？',
    downloadAndInstall: '下载并安装',
    downloading: '正在下载…（{contentLength}）',
    installing: '正在安装…',
    installedTitle: '更新已安装',
    installedBody: '重启应用以完成更新。',
    restartNow: '立即重启',
    later: '稍后',
    failedTitle: '检查更新失败',
    failedBody: '更新检查未完成：{detail}',
    unavailableBody: '当前构建未配置更新通道。',
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
    addMore: '添加文件',
    lockedTag: '压缩中',
    lockedHint: '压缩进行中，队列已锁定',
  },
  settings: {
    eyebrow: '压缩参数预设',
    compressionMode: '压缩模式',
    recommendedBadge: '推荐',
    recommendedShort: '推荐',
    presetGroupLabel: '压缩预设',
    advancedToggle: '高级设置',
    quality: '图片质量',
    qualityHint: 'JPEG 重编码质量（1–100），越低体积越小',
    maxEdge: '图片尺寸上限',
    maxEdgeHint: '图片最长边超过此上限会被缩小；数值为相对原文档最大边长的百分比',
    optimizeImages: '优化图片',
    optimizeImagesHint: '重新编码文档中的图片以减小体积；关闭后图片保持原样',
    compressStreams: '压缩流',
    compressStreamsHint: '对内容流等 PDF 对象启用无损压缩（Flate）',
    stripMetadata: '移除元数据',
    stripMetadataHint: '清除元数据中的作者、制作工具等文档信息',
    colorMode: '色彩模式',
    colorModeColor: '彩色',
    colorModeGray: '灰度',
    colorModeBw: '黑白（G4）',
    subsetFonts: '字体子集化',
    subsetFontsHint: '把内嵌字体裁剪为文档实际使用的字形，减小体积',
    outputDir: '输出目录',
    outputDirDefault: '与原文件相同',
    outputDirBrowse: '选择',
    targetMode: '目标大小',
    targetSizeUnit: '单位',
    targetSizeOff: '关闭',
    targetSizeInvalid: '请输入 0.1 到 2048 之间的数字（MB）。',
    savePreset: '保存预设',
    resetPresets: '恢复默认',
    applyToAll: '应用到全部',
    applyToAllHintSingle: '至少添加两个文件后才能应用到全部。',
    presetSavedTitle: '预设已保存',
    presetSavedBody: '当前图片质量与最大边长已保存到该预设。',
  },
  queue: {
    count: '{count} 项',
    overrideTag: '已覆盖推荐参数',
    delete: '删除',
    contextMenuLabel: '队列项操作',
    openCompressedFile: '打开压缩后 PDF',
    openCompressedFolder: '打开压缩文件所在文件夹',
    pageCount: '{count} 页',
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
    startCompressionAll: '压缩全部（{count}）',
    compressSelected: '仅压缩当前文件',
    recompress: '重新压缩',
    cancel: '取消压缩',
    outputLocked: '压缩进行中，输出目录已锁定。',
    outputDirDesktopOnly: '仅支持在 Tauri 桌面应用中选择输出目录。',
    report: {
      savedValue: '节省 {size}',
      elapsedValue: '耗时 {time}',
      imagesValue: '重压缩 {recompressed} 张图片，跳过 {skipped} 张',
      dedupValue: '合并 {count} 个重复图片',
      streamsValue: '压缩 {count} 个流',
    },
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
      backendNoteTitle: '处理提示',
      restoreSkippedTitle: '队列已恢复',
      restoreSkippedBody: '上次会话中的 {count} 个文件已不存在，已跳过恢复。',
      scanPipelineTitle: '扫描件模式',
      scanPipelineBody: '该文件以扫描内容为主，已使用扫描件管线（强制开启图片与流优化）。',
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
    'analysis.warning.unsupportedImageCodecs': {
      body: '{count} 张图片使用了当前版本无法重编码的编码器（JBIG2、JPX、CCITT）；这些图片将原样保留，且不计入预估。',
    },
    'compress.note.streamDedupe': {
      body: '已合并 {count} 个重复的非图像流对象（内容流、字体、表单）为共享引用。',
    },
    'compress.note.resourcesCleaned': {
      body: '已移除 {count} 个从未被内容流引用的字体/XObject 资源条目。',
    },
    'compress.note.fontsSubsetted': {
      body: '已将 {count} 个内嵌字体子集化到实际使用的字形，减少 {savedKb} KB 字体数据。',
    },
    'analysis.note.encryptedUnlocked': {
      body: '该 PDF 使用了 owner 密码加密，已用空用户密码解锁；压缩导出件将是未加密的。',
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
    'compress.note.decryptedInput': {
      body: '输入文件使用 owner 密码加密，已用空用户密码读取；输出将以未加密形式写出。',
    },
    'compress.warning.outputNotSmaller': {
      body: '优化结果（{bestBytes} 字节）未能小于原文件（{originalBytes} 字节）；未写入任何输出。',
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
    'error.inputTooLarge': {
      body: '文件大小为 {size}，超过了可安全处理的 {limit} 上限。',
    },
    'error.encryptedPdf': {
      body: '该 PDF 使用了不受支持的安全处理器（DRM 加密），无法处理。',
    },
    'error.passwordRequired': {
      body: '该 PDF 需要输入打开密码才能处理。',
    },
    'error.wrongPassword': {
      body: '输入的密码未能解锁该 PDF。',
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
