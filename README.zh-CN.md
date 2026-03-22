# PDF Compressor

语言版本：`README.md`（English）| `README.zh-CN.md`（简体中文）

PDF Compressor 是一个本地优先的桌面 PDF 压缩应用，后端使用 Rust，前端使用 Vue 3 + Tauri。它围绕单文件流程设计，选择一个 PDF，先分析是否值得压缩，再调整一组聚焦的设置，最后导出更轻的副本，不会覆盖原文件。

当前版本：`0.1.0`

作者：`cosct`

## 概览

这个项目面向有选择的 PDF 优化，而不是对整个文件做盲目重写。当前处理流程会尽量保留文本和矢量指令，然后优先优化通常更安全的部分：

- 可处理的嵌入图片流可以重新编码为 JPEG，并在需要时缩小尺寸
- 符合条件的非图片 PDF 流可以进行压缩
- 文档元数据可以移除
- UI 会先执行一次分析，让用户在导出前查看预估收益和推荐预设

本应用以桌面优先、本地优先为前提。本版本没有上传流程、没有云端处理、也没有批量队列。

## 主要特性

### 面向用户的功能

- 单个 PDF 输入，支持拖放、手动输入路径和原生桌面文件选择
- 压缩前预检分析
- 三个预设：`conservative`、`balanced`、`maximum`
- 可调图片质量和最大图片边长
- 可切换图片优化、流压缩和元数据移除
- 结果视图展示输出路径、耗时、体积变化和优化计数
- 提供英文和简体中文界面

### 后端能力

- 通过 `lopdf` 检查 PDF 结构
- 基于纯 Rust 的预检分析，依据页面映射、页面资源和可提取文本进行判断
- 启发式文档分类：`text-native`、`mixed`、`scan-heavy`
- 基于扫描文档置信度推荐预设
- 对暂不支持安全重写的图片流执行安全跳过
- 输出文件命名不会替换原始源文件

## 使用流程

应用采用两阶段后端流程和三步式界面流程。

1. 添加一个 PDF。
2. 运行分析，估计是否值得压缩。
3. 查看建议，调整设置，然后导出新的优化副本。

“先分析再压缩”的规则在 `src/composables/usePdfCompressor.ts` 中强制执行。对于当前源路径，在分析完成前，压缩操作会保持禁用。

## 项目架构

### 高层流程

```text
Vue UI -> Tauri bridge -> Rust commands -> PDF analysis/compression engine -> output PDF
```

### 前端架构

- `src/main.ts`
  - Vue 入口
  - 加载全局样式并挂载带 i18n 的应用

- `src/App.vue`
  - 顶层应用外壳
  - 组合输入、分析、设置和结果面板
  - 将工作流状态映射为面向用户的状态文案

- `src/composables/usePdfCompressor.ts`
  - 工作流状态的单一事实来源
  - 保存所选路径、设置、分析结果、压缩结果、加载状态和错误信息
  - 将后端返回值规范化为前端类型
  - 强制先分析，再压缩

- `src/lib/tauri.ts`
  - Vue 与 Tauri 命令之间的薄桥接层
  - 检测是否可用原生命令
  - 打开桌面文件选择器
  - 监听原生拖放事件
  - 调用 `analyze_pdf` 和 `compress_pdf`

- `src/i18n/index.ts`
  - 初始化 `vue-i18n`
  - 支持 `en` 和 `zh-CN`
  - 在本地存储中保存所选语言

### 原生后端架构

- `src-tauri/src/lib.rs`
  - Tauri 应用入口
  - 注册插件和命令处理器

- `src-tauri/src/commands.rs`
  - 向前端暴露 Tauri 命令
  - 合并并规范化来自前端的压缩设置
  - 当前注册 `analyze_pdf`、`compress_pdf` 和 `compress_scanned_pdf`

- `src-tauri/src/models.rs`
  - Rust 与 Vue 之间序列化传输的共享请求和响应结构

- `src-tauri/src/error.rs`
  - 面向用户失败信息的后端统一错误映射

- `src-tauri/src/pdf/analyzer.rs`
  - 执行预检分析
  - 检查文件大小、精确页数、可提取文本密度和图片/XObject 结构
  - 估计扫描件置信度、图片覆盖率、可能节省比例和推荐预设

- `src-tauri/src/pdf/compressor.rs`
  - 执行对象级 PDF 优化
  - 重新压缩受支持的图片流
  - 压缩符合条件的非图片流
  - 可选移除文档元数据
  - 以生成的新文件名写出输出文件

- `src-tauri/src/pdf/settings.rs`
  - 规范化预设和设置
  - 应用后端默认值并限制可接受范围

## 目录结构

```text
.
├─ src/
│  ├─ main.ts                       # Vue 应用入口
│  ├─ App.vue                       # 主外壳与面板组合
│  ├─ composables/
│  │  └─ usePdfCompressor.ts        # 工作流状态、命令调用、数据规范化
│  ├─ lib/
│  │  └─ tauri.ts                   # 原生桥接、对话框、拖放、命令调用
│  ├─ components/
│  │  ├─ FileIntakePanel.vue        # 源文件路径输入与拖放界面
│  │  ├─ AnalysisPanel.vue          # 分析摘要
│  │  ├─ CompressionSettingsPanel.vue
│  │  ├─ ResultPanel.vue
│  │  └─ AppHeader.vue
│  ├─ i18n/
│  │  └─ index.ts                   # 语言初始化与持久化
│  ├─ locales/
│  │  ├─ en.ts
│  │  └─ zh-CN.ts
│  └─ types/
│     └─ pdf.ts                     # 前端 PDF 工作流类型
├─ src-tauri/
│  ├─ Cargo.toml                    # Rust crate 元数据与原生依赖
│  ├─ tauri.conf.json               # Tauri 产品与打包配置
│  └─ src/
│     ├─ lib.rs                     # Tauri 构建入口
│     ├─ commands.rs                # 命令接口层
│     ├─ models.rs                  # 分析与压缩载荷结构
│     ├─ error.rs                   # 共享后端错误类型
│     └─ pdf/
│        ├─ analyzer.rs             # 预检分析引擎
│        ├─ compressor.rs           # 压缩引擎
│        └─ settings.rs             # 设置规范化
├─ package.json                     # 前端脚本与 JS 依赖
└─ README.md
```

## 环境要求

你需要一套标准的 Vue + Tauri 桌面应用开发环境：

- Node.js 和 npm
- Rust 工具链
- 对应操作系统所需的 Tauri 构建前置依赖

## 开发

### 安装依赖

```bash
npm install
```

### 仅运行前端

```bash
npm run dev
```

这会只启动 Vite 前端，适合做 UI 开发、布局检查和一般前端调试。

浏览器预览模式有这些重要限制：

- 无法使用原生 Tauri 命令
- 原生文件选择器不可用
- 拖放不一定能像桌面壳环境那样提供可用的本地文件路径
- UI 里仍可手动输入路径
- 真正的后端分析和压缩必须在桌面壳中运行

也就是说，预览模式只能在界面层面体验手动路径流程，原生浏览功能必须依赖桌面壳。

### 在开发中运行完整桌面应用

```bash
npm run tauri dev
```

这是主要的端到端开发方式。它会启动 Vue 开发服务器，并打开 `src-tauri/tauri.conf.json` 中定义的 Tauri 桌面壳。

当你需要验证以下能力时，应使用这个模式：

- 原生文件浏览
- 原生拖放
- 后端分析
- 压缩输出生成
- 基于纯 Rust 的 PDF 检查与优化

## 构建与发布

### 构建前端产物

```bash
npm run build
```

这会执行：

- `vue-tsc -b`
- `vite build`

### 构建桌面发布产物

```bash
npm run tauri build
```

发布元数据定义在：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

当前首个版本的元数据为：

- 产品名：`PDF Compressor`
- 版本：`0.1.0`
- 作者：`cosct`
- 标识符：`com.cosct.pdfcompressor`

`src-tauri/tauri.conf.json` 中当前的 Tauri 打包目标为 `all`。

## 运行时细节

### 分析阶段

`src-tauri/src/pdf/analyzer.rs` 中的分析步骤是一次轻量级预检，它帮助 UI 回答一个实际问题，这个文件是否有足够的缩小空间，值得导出一个优化副本。

当前会评估：

- 文件大小
- 从 PDF 页树读取的精确页数
- 来自页面资源和 XObject 的嵌入图片信号
- 当 `lopdf` 能解码时，各页可提取文本的密度
- 基于页面内容操作符和字体资源的结构化文本回退信号
- 扫描文档置信度
- 估计图片覆盖率

后端会据此返回：

- `documentKind`
- `recommendedPreset`
- `estimatedSavingsPercent`
- 给 UI 使用的警告与提示

这些结果是启发式建议，不是对压缩结果的精确保证。

由于这一阶段现在完全基于纯 Rust 且不依赖渲染器，当 PDF 存在异常编码、局部 OCR 图层或复杂资源图时，分析会刻意偏保守。遇到模糊情况时，更可能被归为 `mixed`，而不是被夸大判定为 `scan-heavy`。

### 压缩阶段

`src-tauri/src/pdf/compressor.rs` 中的压缩器工作在 PDF 对象级别。

当前行为包括：

- 仅当图片流看起来可以安全重写时才检查并重新压缩图片
- 受支持的图片内容可以缩小到配置的最大边长
- 重压缩后的图片会编码为 JPEG
- 符合条件的非图片流在尚未压缩时可以被压缩
- 可以从文档信息和根元数据条目中移除元数据

当某个对象不能被安全重写时，压缩器会明确优先保留文本和矢量指令。

## 使用方式

### 第一步，添加一个 PDF

在输入面板中，你可以：

- 将 PDF 拖放到窗口中
- 在 Tauri 桌面壳中使用原生浏览按钮
- 手动粘贴一个 `.pdf` 文件的完整路径

如果当前路径不是以 `.pdf` 结尾，前端中的分析和压缩都会保持禁用。

### 第二步，分析文件

点击 `Analyze PDF` 运行原生分析。

分析视图会展示：

- 源文件大小
- 页数
- 嵌入图片数量
- 检测到的文档类型匹配
- 预计节省比例
- 推荐预设
- 警告和说明

推荐预设会同步显示在设置面板中。

### 第三步，调整设置并压缩

设置面板提供以下选项：

- 预设
- 图片质量
- 最大图片边长
- 是否优化图片
- 是否压缩流
- 是否移除元数据

分析完成后，点击 `Compress PDF` 生成优化后的副本。

### 工作流状态

前端在 `src/types/pdf.ts` 和 `src/composables/usePdfCompressor.ts` 中定义了这些状态：

- `idle`
- `selected`
- `analyzing`
- `ready`
- `compressing`
- `success`
- `error`

## 输出行为

应用会在原始 PDF 所在目录旁生成一个新文件，不会覆盖输入文件。

当前命名格式为：

```text
<original-name>__optimized-<preset>.pdf
```

示例：

```text
report.pdf
report__optimized-balanced.pdf
```

如果该名称已存在，后端会继续追加数字后缀，例如 `-1`、`-2`，直到找到可用文件名。

结果面板会报告：

- 输出路径
- 耗时
- 原始大小
- 压缩后大小
- 节省字节数
- 节省百分比
- 已优化图片数
- 已跳过图片数
- 已压缩流数量

如果输出文件并没有比原文件更小，UI 会明确给出提示，方便你自行检查结果后决定保留哪一份。

## 本地化

本地化初始化位于 `src/i18n/index.ts`。

当前语言：

- `en`
- `zh-CN`

行为如下：

- 先从本地存储读取语言设置
- 否则检查浏览器语言
- 中文浏览器语言默认使用 `zh-CN`
- 其他情况回退到 `en`
- 所选语言会保存到本地存储键 `pdf-compressor-locale`

当前翻译内容位于：

- `src/locales/en.ts`
- `src/locales/zh-CN.ts`

## 故障排查

### 浏览按钮不可用

这在浏览器预览模式中是正常现象。原生浏览功能只在 Tauri 桌面壳中可用。

你可以这样处理：

- 运行 `npm run tauri dev`
- 手动把完整的 `.pdf` 路径粘贴到输入框中

### 分析或压缩提示路径无效

请检查：

- 文件确实存在
- 路径以 `.pdf` 结尾
- 当前桌面会话下应用有权限访问该文件

后端在处理前会同时校验文件是否存在，以及扩展名是否为 `.pdf`。

### 压缩完成，但文件没有变小

这种情况可能出现在：

- 文本原生 PDF
- 已经优化过的 PDF
- 几乎没有嵌入图片的文件
- 被压缩器有意跳过的图片过滤器类型

如果你能接受额外有损压缩，可以尝试更强的预设或更低的图片质量。

### 部分图片被跳过

这对某些图片对象是预期行为。当前后端会跳过尚不能安全重写的流，包括但不限于：

- 透明度或图片蒙版
- `JPXDecode`、`JBIG2Decode`、`CCITTFaxDecode`、`Crypt` 等不受支持的过滤器
- 不受支持的原始图片布局
- 无法安全解码的图片数据

### 大型 PDF 处理较慢

当页数较多时，分析器已经会给出提醒。大型 PDF 或图片很多的 PDF 会更慢，因为后端必须检查，必要时还要重写大量对象。

## 限制

本版本刻意保持聚焦，因此有一些明确限制。

- 应用一次只处理一个 PDF，没有批处理流程。
- 压缩收益是启发式估计，预计节省比例只是建议，不是保证。
- 分析阶段是纯 Rust、基于结构的判断，不会渲染页面，所以对某些 PDF 来说，文本提取和扫描件识别只能做到近似判断。
- 某些嵌入图片格式，以及受保护或结构复杂的内容，会被刻意跳过，以避免破坏文档。
- 当前只尝试安全的对象级优化，不承诺对所有 PDF 结构做激进重写。
- 前端依赖桌面壳来提供原生命令。浏览器预览模式适合做 UI 工作，不适合完整 PDF 处理。
- 后端命令接口中存在 `compress_scanned_pdf`，但当前 Vue 工作流实际使用的是 `analyze_pdf` 和 `compress_pdf`。

## 技术栈

- Vue 3
- TypeScript
- Vite
- Tauri 2
- Rust
- `lopdf`
- `image`
- `printpdf`

## v0.1.0 发布说明

`0.1.0` 建立了这个项目首个完整的桌面工作流：

- Vue + Tauri 应用外壳
- 原生 PDF 路径输入
- PDF 预检分析
- 对象级压缩流程
- 英文和简体中文本地化界面
- `PDF Compressor` 的桌面打包元数据
