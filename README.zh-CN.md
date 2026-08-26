# PDF Compressor

语言版本：`README.md`（English）| `README.zh-CN.md`（简体中文）

PDF Compressor 是一个本地优先的桌面 PDF 压缩应用，后端使用 Rust，前端使用 Vue 3 + Tauri。它支持多文件队列工作流：添加一个或多个 PDF，由应用自动分析每个文件，按文件或全局调整设置，然后导出更轻的副本，不会覆盖原文件。

当前版本：`0.2.0`

作者：`cosct`

## 概览

这个项目面向有选择的 PDF 优化，而不是对整个文件做盲目重写。当前处理流程会尽量保留文本和矢量指令，然后优先优化通常更安全的部分：

- 可处理的嵌入图片流可以重新编码为 JPEG，并在需要时缩小尺寸
- 带透明度（`/SMask`）的图片会在保留 Alpha 通道的前提下重写
- 字节级相同的重复图片（Logo、印章）会被无损合并为共享引用
- 目标大小模式会在质量/分辨率参数空间中搜索，直到输出满足字节预算（UI、CLI `--target-size` 与 IPC 均可使用）——二分查找“能适配预算的最高质量”，把剩余预算花在画质上；只有整个质量范围都失败时才收缩图片边长
- 符合条件的非图片 PDF 流可以进行压缩
- 文档元数据可以移除
- UI 会先执行一次分析，让用户在导出前查看预估收益和推荐预设；预估只统计压缩器真正能处理的图片（JBIG2/JPX/CCITT 编码的图片会如实报告为“原样保留”，而不是被算进预计收益）
- 安全护栏：需要真实密码的加密文档会在入口被拒绝；仅 owner 密码（空用户密码可读）的文件会被解锁并以未加密形式重写；优化结果若不能小于原文件，则不会写入磁盘

本应用以桌面优先、本地优先为前提。没有上传流程，没有云端处理。桌面版支持本地队列，可以批量分析和压缩多个 PDF。

### 引擎行为说明

- **图片密集文档**：当 PDF 含有大量图片对象（24 张及以上）时，小流跳过启发式会被解除——数百张紧凑扫描图的累计收益是真实的，而逐图“只有更小才替换”的规则仍保证不会变大。显式请求灰度转换时同样解除跳过。
- **右键快速模式绝不留下更差的文件**：见[后台模式](#后台模式右键快速压缩)。
- **CJK 文档**：文本、嵌入字体子集与 ToUnicode 映射按原样保留（对象只被搬运，不被重新解释）；已在 42 页中文文档与日文样本上端到端验证——字符抽取多重集合一致，渲染像素级可比。

## 安装

### Arch Linux（AUR）

AUR 源码包文件位于 [`aur/pdf-compressor/`](aur/pdf-compressor/)。包发布到 AUR 后，可使用任意 AUR 助手安装：

```bash
yay -S pdf-compressor      # 或：paru -S pdf-compressor
```

若要基于提供的文件在本地构建，请参考 [`aur/pdf-compressor/README.md`](aur/pdf-compressor/README.md)，其中说明了如何替换源码校验和、重新生成 `.SRCINFO`，以及在发布归档之前创建本地源码 tarball 进行测试。

### 其他平台

目前尚未发布预编译安装包。请按照[构建与发布](#构建与发布)从源码构建。在 Windows 上会生成 NSIS 安装包和便携版可执行文件。

## v0.2.0 新增内容

### Fluent Design 界面重设计

整个界面按照 Microsoft Fluent Design System 原则重新设计：

- **深色与浅色主题** — 应用支持三种主题模式：深色、浅色、跟随系统（读取操作系统偏好）。首次启动默认为跟随系统。主题选择保存在本地存储中，并在首次渲染前同步应用，避免视觉闪烁。
- **亚克力材质** — 顶部栏使用了亚克力 backdrop-filter 效果（模糊、半透明），与 Windows 11 的设计语言保持一致，营造层次感。
- **Fluent 设计令牌** — 所有颜色、间距、排版、圆角、阴影和过渡效果均以 CSS 自定义属性（custom properties）定义，遵循 WinUI 3 / Fluent 2 令牌规范。深色和浅色主题各有一套完整的语义令牌。
- **PDF 上传作为主视图** — 上传/队列面板现在是左侧主区域，占据大部分屏幕空间。设置和活动面板放在右侧较窄的侧栏中，让拖放上传区成为最显眼的核心元素。
- **响应式布局** — 双栏网格在窄屏上自动折叠为单栏。侧栏在宽屏上使用 sticky 定位，在滚动长队列时始终可见。
- **自定义窗口边框** — 应用使用无边框窗口，集成了自定义标题栏，包含最小化、最大化/还原和关闭控件，与主题和语言切换器并排。
- **统一图标** — 每个面板标题都带有小型内联 SVG 图标，增强视觉锚点。拖放区域有更大的上传图标以提高可发现性。
- **开关切换** — 布尔设置（优化图片、压缩流、移除元数据）使用 Fluent 风格的开关切换，取代了原始的复选框。
- **启动画面** — 在主窗口和 Vue 应用初始化期间显示轻量启动画面窗口，通过 `app_ready` 命令关闭。
- **错误通知** — 后端错误和对话框失败以浮动通知卡片形式展示，按级别着色（危险、警告、成功），可逐条关闭。

### 压缩速度优化

Rust 压缩引擎进行了针对性的性能改进：

- **CatmullRom 缩放滤波器** — 用 `CatmullRom`（双三次插值）替换了 `Triangle` 作为最终缩放滤波器。CatmullRom 比 `Lanczos3` 快约 2 倍，对于 JPEG 输出几乎无法区分质量差异，且比 `Triangle` 更锐利。
- **更早触发两阶段缩放** — 两阶段缩放策略（先用 Nearest 粗缩，再用 CatmullRom 精缩）的触发阈值从 800 万像素降低到 400 万像素。这使得中等大小的图片（如 2000×2000）也能受益于快速的第一阶段，减少总体缩放时间。
- **更宽的边长容差** — `RESIZE_EDGE_TOLERANCE` 从 1.05 提高到 1.08。仅略微超出目标边长的图片不再被缩放，避免了为微不足道的尺寸减小而进行的解码+缩放+编码往返。
- **更大的工作通道缓冲** — 并行图片处理的通道缓冲区现在是工作线程数的 4 倍（原来是 2 倍），减少了生产者线程的阻塞，提高了流水线吞吐量。
- **更低的并行阈值** — 并行图片处理现在在 3 张以上图片时即触发（原来需要 4 张），使较小的 PDF 也能利用多核处理。
- **更小的流压缩最低字节数** — 64 字节以上的非图片流（原来是 128 字节）现在可以尝试 deflate 压缩，捕获更多短重复流。
- **更低的微型 JPEG 跳过阈值** — 6 KB 以下的 JPEG 流（原来是 8 KB）直接跳过，对真正微小的图片更积极地应用快速路径。
- **更高的小流阈值** — `SMALL_IMAGE_STREAM_BYTES` 阈值提高到 64 KB（原来是 48 KB），当紧凑 JPEG 已在目标尺寸内时可以跳过更多。
- **FlateDecode 检测** — 流过滤器分析现在会跟踪 `FlateDecode` 的存在，为将来的已压缩流优化打下基础。

### 主题系统架构

主题系统实现为 Vue composable（`src/composables/useTheme.ts`）：

- **响应式状态** — `themePreference`（ref）跟踪用户的选择：`'dark'`、`'light'` 或 `'system'`。`resolvedTheme`（computed）将 `'system'` 解析为实际的操作系统偏好。
- **系统默认** — 首次启动未存储偏好时，主题默认为 `'system'`，即跟随操作系统的深色/浅色设置。
- **DOM 同步** — watcher 在解析主题变化时，将 `data-theme` 属性和 `color-scheme` CSS 属性应用到 `<html>` 元素。
- **系统偏好监听** — composable 监听 `prefers-color-scheme` 媒体查询变化，当模式设为"跟随系统"时，切换操作系统主题会立即更新界面。
- **防闪烁** — `index.html` 中的同步 `<script>` 块在任何 CSS 或 Vue 代码加载之前，从 localStorage 读取存储的主题并应用 `data-theme` 属性。CSS `prefers-color-scheme` 媒体查询作为脚本执行前的额外回退。
- **持久化** — 主题偏好保存在 localStorage 的 `pdf-compressor-theme` 键下。

### 预设持久化

用户自定义的预设配置通过 `src/config/presets.ts` 和 Rust `commands.rs` 命令层持久化到磁盘：

- **存储位置** — 后端优先写入安装目录；当安装目录不可写时（如 `Program Files`），回退到系统应用配置目录（如 `AppData/Roaming/pdf-compressor`）。
- **原子写入** — 配置先写入 `.tmp` 临时文件再重命名，避免写入中断导致损坏。
- **前端缓存** — 已加载的配置在内存中缓存，避免会话期间重复读取磁盘。
- **重置** — 清除用户覆盖会删除配置文件并恢复内置默认值。

## 主要特性

### 面向用户的功能

- 多文件 PDF 队列，支持拖放和原生桌面文件选择
- 压缩前自动预检分析
- 深色、浅色和跟随系统主题 — 首次启动默认跟随系统偏好
- 三个预设：`conservative`、`balanced`、`maximum`，另有 `custom` 自定义
- 可调图片质量和最大图片边长（百分比方式，基于分析返回的参考边长）
- 开关切换：图片优化、流压缩、元数据移除
- 自定义预设保存、逐预设用户覆盖与恢复默认
- 一键将设置应用到所有队列文件
- 可选择压缩输出目录
- 队列感知的活动视图，展示输出路径、进度、体积变化和优化计数
- 通过系统处理器打开或显示压缩输出文件
- 支持按任务取消压缩
- 错误浮动通知，展示后端和对话框失败信息
- 初始化加载时显示启动画面
- 自定义窗口边框（无边框 + 集成标题栏控件）
- 生产构建同时输出便携版可执行文件和 NSIS 安装包
- 英文和简体中文界面

### 后端能力

- 通过 `lopdf` 检查 PDF 结构
- 基于纯 Rust 的预检分析，依据页面映射、页面资源和可提取文本进行判断
- 启发式文档分类：`text-native`、`mixed`、`scan-heavy`
- 基于扫描文档置信度推荐预设
- 大型 PDF 的抽样页面检查
- 对暂不支持安全重写的图片流执行安全跳过
- 优化调度的并行图片重压缩（最大优先、更宽通道缓冲）
- 两阶段缩放与 CatmullRom 滤波器兼顾速度和质量
- 输出文件命名不会替换原始源文件
- 用户预设配置持久化，优先写入安装目录，不可写时回退到系统配置目录
- 压缩任务注册表，支持取消标志传播
- 系统处理器集成，用于打开和显示输出文件
- 启动画面窗口管理（启动时显示，应用就绪后关闭）

## 使用流程

应用采用两阶段后端工作流和三步 UI 流程。

1. 通过拖放、浏览按钮或原生文件选择器添加一个或多个 PDF。
2. 由应用分析每个文件并推荐预设。
3. 查看所选文件，按需调整设置，导出优化副本。

"先分析再压缩"的规则在 `src/composables/usePdfCompressor.ts` 中强制执行。压缩会等待每个队列文件完成分析后，才把该文件交给后端压缩器。

## 项目架构

### 高层流程

```text
Vue UI -> Tauri bridge -> Rust commands -> PDF analysis/compression engine -> output PDF
```

### 前端架构

- `src/main.ts` — Vue 入口；加载全局样式并挂载带 i18n 的应用
- `src/App.vue` — 顶层应用外壳；双栏布局：上传面板（主区域）+ 侧栏（设置、活动）；将工作流状态映射为用户可见的状态文案
- `src/composables/useTheme.ts` — 主题管理 composable（深色 / 浅色 / 跟随系统），持久化存储，DOM 同步，监听系统主题变化
- `src/composables/usePdfCompressor.ts` — 工作流状态的单一事实来源；管理任务、设置、分析结果、压缩结果、加载状态和错误；将后端载荷规范化为前端类型；强制"先分析再压缩"；用工作线程池管理并发压缩
- `src/composables/useErrorToasts.ts` — 错误通知状态管理：去重后的通知队列，由 `ErrorToastViewport` 呈现
- `src/composables/backendMessages.ts` — 后端消息适配层：净化后端载荷，做运行时 tone/phase 校验并本地化
- `src/lib/tauri.ts` — Vue 与 Tauri 命令之间的桥接层；检测原生命令是否可用；打开桌面文件/目录选择器；监听原生拖放事件；调用 `analyze_pdf`、`compress_pdf`、`cancel_compression` 和预设配置命令；窗口管理（最小化、最大化、关闭、拖动）
- `src/lib/bindings.ts` — 由 tauri-specta 生成的类型化 IPC 层（命令与载荷类型；`export_bindings` 测试负责再生成）
- `src/config/presets.ts` — 预设配置管理（加载、保存、清除），合并内置默认值与用户覆盖，缓存已加载配置
- `src/config/preset-defaults.json` — 各压缩预设的内置默认值（质量、最大图片尺寸百分比）
- `src/utils/compressionSettings.ts` — 图片质量、尺寸百分比和像素值的夹紧与规范化；基于参考边长把百分比转换为绝对像素值
- `src/utils/format.ts` — 格式化工具（字节、百分比、毫秒、路径）
- `src/i18n/index.ts` — 国际化初始化，支持 `en` 和 `zh-CN`，语言选择保存在 localStorage

### PDF 引擎（`crates/pdf-core`）

- `crates/pdf-core/src/lib.rs` — 引擎 crate 入口，汇聚分析、压缩、模型与错误模块
- `crates/pdf-core/src/pdf/analyzer.rs` — 预检分析引擎（页面抽样、文本密度、图片信号）
- `crates/pdf-core/src/pdf/compressor.rs` — 对象级 PDF 优化引擎（图片重压缩、流压缩、元数据移除）
- `crates/pdf-core/src/pdf/settings.rs` — 设置规范化，应用后端默认值并限制范围
- `crates/pdf-core/src/models.rs` — Rust 与调用方之间序列化传输的分析/压缩载荷结构
- `crates/pdf-core/src/error.rs` — 引擎统一错误类型，带 i18n 兼容的错误码
- `crates/pdf-core/src/bin/pdf-compressor-cli.rs` — `pdf-compressor-cli` 命令行工具（analyze / compress / 后台 quick 模式，支持桌面通知）
- `crates/pdf-core/benches/` — criterion 基准测试（压缩管线、JPEG 编码器对比）
- `crates/pdf-core/fuzz/fuzz_targets/pipeline.rs` — cargo-fuzz 目标

### 桌面壳（`src-tauri`）

- `src-tauri/src/lib.rs` — Tauri 应用入口；注册插件，管理启动画面窗口，注册命令处理器
- `src-tauri/src/commands.rs` — Tauri 命令接口层；合并并规范化来自前端的压缩设置；预设配置持久化；压缩任务注册与取消；通过系统处理器打开/显示文件

## 目录结构

```text
.
├─ public/
│  └─ splash.html                   # 应用初始化时显示的启动画面
├─ src/
│  ├─ main.ts                       # Vue 应用入口
│  ├─ App.vue                       # 主外壳：双栏布局
│  ├─ composables/
│  │  ├─ usePdfCompressor.ts        # 工作流状态、命令调用、数据规范化
│  │  ├─ useTheme.ts                # 深色/浅色/系统主题管理
│  │  ├─ useErrorToasts.ts          # 去重错误通知队列
│  │  ├─ backendMessages.ts         # 后端消息净化与本地化
│  │  └─ __tests__/                 # Vitest 单元测试
│  ├─ lib/
│  │  ├─ tauri.ts                   # 原生桥接、对话框、拖放、命令调用
│  │  └─ bindings.ts                # tauri-specta 生成的类型化 IPC 层
│  ├─ config/
│  │  ├─ presets.ts                 # 预设配置管理、持久化、默认值合并
│  │  └─ preset-defaults.json       # 各预设的内置默认值
│  ├─ components/
│  │  ├─ PdfUploadPanel.vue         # 队列上传与拖放界面（主视图）
│  │  ├─ CompressionSettingsPanel.vue # 预设网格、滑块、开关
│  │  ├─ ActivityPanel.vue          # 当前任务状态、压缩按钮、指标
│  │  ├─ AppHeader.vue              # 品牌、主题切换、语言切换、窗口控件
│  │  └─ ErrorToastViewport.vue     # 浮动错误/警告通知
│  ├─ i18n/
│  │  └─ index.ts                   # 语言初始化与持久化
│  ├─ locales/
│  │  ├─ en.ts
│  │  └─ zh-CN.ts
│  ├─ utils/
│  │  ├─ compressionSettings.ts     # 图片质量/尺寸夹紧与像素计算
│  │  └─ format.ts                  # 格式化工具（字节、百分比、路径、毫秒）
│  └─ types/
│     └─ pdf.ts                     # 前端 PDF 工作流类型
├─ crates/
│  └─ pdf-core/                     # 纯 Rust PDF 引擎 crate
│     ├─ Cargo.toml
│     ├─ src/
│     │  ├─ lib.rs                  # 引擎 crate 入口
│     │  ├─ models.rs               # 分析/压缩载荷结构
│     │  ├─ error.rs                # 引擎错误类型（含 i18n 错误码）
│     │  ├─ pdf/
│     │  │  ├─ analyzer.rs          # 预检分析引擎
│     │  │  ├─ compressor.rs        # 压缩编排（文档遍历、批量调度）
│     │  │  ├─ encode.rs            # 单图编解码（跳过启发式、缩放、JPEG、软蒙版）
│     │  │  ├─ search.rs            # 目标大小搜索状态（逐图缓存、探测与物化）
│     │  │  ├─ target_size.rs       # 目标大小搜索调度与入口
│     │  │  ├─ workers.rs           # 共享有界作用域线程池
│     │  │  ├─ settings.rs          # 设置规范化
│     │  │  └─ tests.rs             # 管线集成测试
│     │  ├─ testutil.rs             # 确定性夹具生成器（测试/基准/示例共用）
│     │  └─ bin/
│     │     └─ pdf-compressor-cli.rs # pdf-compressor-cli 命令行工具
│     ├─ benches/                   # criterion 基准测试
│     └─ fuzz/                      # cargo-fuzz 目标（pipeline）
├─ src-tauri/
│  ├─ Cargo.toml                    # Rust crate 元数据与原生依赖
│  ├─ tauri.conf.json               # Tauri 产品与打包配置
│  └─ src/
│     ├─ lib.rs                     # Tauri 构建入口、启动画面管理
│     ├─ main.rs                    # 桌面程序入口
│     └─ commands.rs                # 命令接口层、预设配置、任务注册
├─ aur/
│  └─ pdf-compressor/                # Arch Linux（AUR）源码包（PKGBUILD、.SRCINFO）
├─ scripts/
│  ├─ sync-version.mjs              # 版本号同步（package.json -> tauri.conf.json / Cargo.toml）
│  └─ postbuild-portable.mjs        # 便携版可执行文件后处理
├─ package.json                     # 前端脚本与 JS 依赖
├─ README.md
└─ README.zh-CN.md
```

## 环境要求

标准的 Vue + Tauri 桌面应用开发环境：

- Node.js 和 npm
- Rust 工具链（MSRV：`pdf-core` 1.88，桌面应用 1.93 — 由 CI 强制执行）
- 对应操作系统所需的 Tauri 构建前置依赖

在 Arch Linux 上，系统依赖为 `webkit2gtk-4.1` 和 `gtk3`（构建还需 `cargo`、`nodejs`、`npm` 和 `pkgconf`）；完整列表以 `aur/pdf-compressor/PKGBUILD` 为准。其他发行版需要安装等价的 WebKit2GTK 4.1 与 GTK 3 软件包。

## 开发

> 面向贡献者的整合指南（环境搭建、架构、测试、规范与打包）见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

### 安装依赖

```bash
npm install
```

### 仅运行前端

```bash
npm run dev
```

只启动 Vite 前端，适合 UI 开发、布局检查和一般前端工作。

浏览器预览模式的重要限制：

- 原生 Tauri 命令不可用
- 原生文件选择器被禁用
- 拖放可能拿不到可用的桌面文件路径
- 真正的后端分析和压缩需要桌面壳

### 运行完整桌面应用

```bash
npm run tauri dev
```

启动 Vue 开发服务器和 Tauri 桌面壳。验证原生文件浏览、拖放、后端分析、压缩输出和主题切换时使用此模式。

### 测试

```bash
npm test                    # 前端单元测试（Vitest）
cargo test --workspace      # Rust 单元 + 管线集成测试
cargo bench -p pdf-core     # 压缩基准测试（criterion）
```

Rust 代码是一个 Cargo workspace：`crates/pdf-core` 是纯 PDF 引擎（分析器、压缩器、模型、`pdf-compressor-cli` 二进制、基准测试和 cargo-fuzz 目标），`src-tauri` 是桌面壳。Rust 测试套件中有一个 `export_bindings` 测试，负责重新生成 `src/lib/bindings.ts`（由 tauri-specta 产出的类型化 IPC 层）。每当 Tauri 命令签名发生变化，运行 `cargo test --workspace` 并把再生成后的绑定随改动一起提交。

另有一个小型 CLI 可供 shell 使用和调试：

```bash
cargo run -p pdf-core --bin pdf-compressor-cli -- analyze <file.pdf>
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --preset maximum
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --target-size 5MB
```

### 后台模式（右键快速压缩）

`pdf-compressor-cli quick` 是文件管理器集成背后的无界面模式：在原文件旁生成压缩副本（沿用 `__optimized-<preset>` 命名），比原文件大时自动丢弃输出，向 stdout 输出 JSON 摘要，并在检测到 `notify-send` 时发送桌面通知：

```bash
pdf-compressor-cli quick file1.pdf file2.pdf          # 均衡预设
pdf-compressor-cli quick --preset maximum scans.pdf  # 最大化压缩
pdf-compressor-cli quick --grayscale book-scan.pdf   # 黑白扫描件最佳
pdf-compressor-cli quick --target-size 5MB report.pdf --no-notify
```

在 KDE Plasma 上，Arch 软件包会安装 Dolphin 服务菜单（`packaging/servicemenus/pdf-compressor.desktop` → `/usr/share/kio/servicemenus/`）：右键 PDF 即可看到「PDF 压缩」子菜单，提供均衡 / 最大化 / 灰度 / 目标大小四种动作，全程不打开 GUI 窗口。加密 PDF 会在入口处以 `error.encryptedPdf` 拒绝；quick 模式绝不留下比原文件更大的输出。

PDF 引擎还有 cargo-fuzz 目标（`crates/pdf-core/fuzz`）— 在 `crates/pdf-core` 目录下运行 `cargo +nightly fuzz run pipeline`。

引擎的变异测试使用 cargo-mutants（CI 每周运行，也可从 *Mutation tests* 工作流手动触发）：

```bash
cargo mutants               # 在 crates/pdf-core 下运行；报告输出到 mutants.out/
```

### 版本管理

`package.json` 是应用版本的单一事实来源。提升版本号后运行：

```bash
npm run sync-version
```

它会把版本号同步到 `src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml`。

## 构建与发布

### 构建前端产物

```bash
npm run build
```

### 构建桌面发布产物

```bash
npm run tauri build
```

这会生成 NSIS 安装包。若需要同时输出便携版（免安装）可执行文件：

```bash
npm run tauri:build
```

便携版输出路径：`target/release/bundle/PDF-Compressor-portable.exe`。

便携版需要 Windows 10 21H2+ 或 Windows 11（这些系统已预装 WebView2）。

发布元数据：

- 产品名：`PDF Compressor`
- 版本：`0.2.0`
- 作者：`cosct`
- 标识符：`com.cosct.pdfcompressor`

### Linux 打包（Arch / AUR）

针对基于 Arch 的发行版，通过 `aur/pdf-compressor/` 中的 AUR 源码包提供打包支持。`PKGBUILD` 会执行 `npm ci && npm run build`，用 `cargo build --release --locked` 构建发布二进制，并将其作为 `/usr/bin/pdf-compressor` 连同桌面入口和图标一起安装。用法见[安装](#安装)，发布流程见 `aur/pdf-compressor/README.md`。

## 运行时细节

### 分析阶段

`crates/pdf-core/src/pdf/analyzer.rs` 中的分析步骤是一次轻量级预检。会评估文件大小、来自 PDF 页面树的精确页数、来自页面资源和 XObject 的嵌入图片信号、抽样页面的可提取文本密度、来自页面内容操作符和字体资源的结构化文本回退信号、扫描文档置信度和估计图片覆盖率。返回 `documentKind`、`recommendedPreset`、`estimatedSavingsPercent` 和提示信息。这是启发式建议，不是精确保证。

### 压缩阶段

`crates/pdf-core/src/pdf/compressor.rs` 中的压缩器工作在 PDF 对象级别：

- 图片流经过检查，仅在安全时才重压缩
- 受支持的图片使用两阶段策略缩放（Nearest + CatmullRom）
- 重压缩后的图片编码为 JPEG
- 符合条件的非图片流可以进行 deflate 压缩
- 可以移除文档信息和根元数据条目中的元数据
- 明确优先保留文本和矢量指令

### 主题系统

主题系统使用 CSS 自定义属性，深色和浅色各有一套完整的令牌集。主题切换即时生效，无需刷新页面。`<html>` 上的 `data-theme` 属性控制激活哪套令牌。首次启动时，主题默认跟随操作系统偏好（`system` 模式）。`index.html` 中的同步脚本在启动时防止主题闪烁，CSS `prefers-color-scheme` 媒体查询作为额外回退。

## 输出行为

应用在原始 PDF 所在目录生成新文件，不覆盖输入文件。

命名格式：`<原始文件名>__optimized-<预设>.pdf`

如果文件名已存在，后端追加数字后缀（`-1`、`-2` 等）直到找到可用名称。

## 本地化

当前语言：`en` 和 `zh-CN`

- 优先从 localStorage 读取
- 回退到浏览器语言检测
- 中文浏览器默认使用 `zh-CN`
- 其他回退到 `en`

## 故障排查

### 浏览按钮不可用

浏览器预览模式中的正常现象。运行 `npm run tauri dev` 以使用原生浏览。

### 压缩完成但文件没有变小

可能原因：文本原生 PDF、已优化过的文件、几乎没有嵌入图片、不支持的图片过滤器。可尝试更强预设或更低图片质量。

### 部分图片被跳过

预期行为。后端会跳过不能安全重写的流（透明度、蒙版、JPX/JBIG2/CCITT/Crypt 过滤器、不支持的颜色空间）。

### 大型 PDF 处理较慢

页数多时分析器会给出提醒。大型或图片密集的 PDF 需要检查和重写大量对象。存在多张图片时，压缩器会使用并行工作线程。

## 限制

- 压缩收益基于启发式，预估节省比例只是建议
- 分析是纯 Rust 结构式判断，不渲染页面
- 某些嵌入图片格式和受保护结构会被刻意跳过
- 只尝试安全的对象级优化
- 前端依赖桌面壳提供原生命令
- 后端存在 `compress_scanned_pdf` 命令但当前 Vue 工作流未使用

## 技术栈

- Vue 3 + TypeScript + Vite
- Tauri 2 + Rust
- `lopdf`（PDF 解析与写入）
- `image`（图片解码与缩放）
- `jpeg-encoder`（SIMD JPEG 重编码）
- `vue-i18n`（国际化）

## 版本历史

### v0.2.0（2026-04-16）

- Fluent Design 界面全面重设计，支持深色/浅色/跟随系统主题
- 首次启动默认跟随系统偏好（不再硬编码深色）
- PDF 上传面板升级为主视图，改进布局
- 亚克力效果的顶部栏
- 自定义窗口边框，集成标题栏控件
- 初始化加载启动画面
- 压缩速度优化：CatmullRom 缩放、更早的两阶段阈值、更宽的通道缓冲、更低的并行阈值
- 用户预设持久化：逐预设覆盖写入磁盘，安装目录优先策略
- 支持按任务取消压缩
- 可选择压缩输出目录
- 通过系统处理器打开和显示压缩输出文件
- 错误浮动通知视口
- 百分比式最大图片尺寸滑块，基于分析返回的参考边长
- 布尔设置使用开关切换
- 面板内联 SVG 图标
- 生产构建同时输出便携版可执行文件和 NSIS 安装包
- 更新 README 文档，详细说明架构和变更

### v0.1.0（2026-03-27）

- 首个桌面工作流：Vue + Tauri 应用外壳、原生 PDF 输入、预检分析、对象级压缩、英文和中文界面
