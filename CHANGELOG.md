# 更新日志 / Changelog

本项目的所有显著变更都记录在此文件中。格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。日期为提交日期。

## [未发布]

### 新增

- **JPX（JPEG 2000）输入解码**（feature `jpx`，默认关闭以保持默认构建纯 Rust；桌面 release 安装包与 AUR 包开启）：PDF 内嵌的 JPEG 2000 图片（J2K 裸码流与 JP2 容器两种形态）现可解码转码为 JPEG/G4 压缩；1/3/4 分量分别按灰度/RGB/CMYK 处理。实现经 `jpeg2k`（OpenJPEG 安全封装）——vendored OpenJPEG 用 `cc` 静态编译链接，**无需 cmake、无需在安装包携带运行时动态库**，三平台打包风险由此消除（2026-09 spike 结论）。unsafe C 面配专职 fuzz target（`cargo +nightly fuzz run jpx`，CI 45s 冒烟）；形状门禁照 CCITT 模式在 `stream_filter_info` 收口，分析器预估自动跟随
- **CMYK 图片支持**：ICC N=4、`DeviceCMYK`、CMYK 基 Indexed 的图片经油墨减色转换（`(1-cmy)×(1-k)`）转为 RGB 后走既有 JPEG 管线；重建流声明 `DeviceRGB`（CMYK profile 不随行）；带 `/Decode` 映射数组的 CMYK 保持跳过（防静默偏色）。保真度由 PSNR 门禁钉住（梯度夹具实测 ≈46dB）
- **2GiB 输入上限的友好提示**：专用错误码 `error.inputTooLarge`（GUI 双语文案就位），携带人性化体积（如 "2.1 GB"）而非原始字节数
- **内存护栏**：目标大小搜索的缓存预算核算扩展到 alpha/G4 产物（原先只算彩色平面），超限两级淘汰；多图大文档搜索的进程内存峰值测试护栏（Linux，实测 ≈390MB / 上限 2GB）；worker 池按编解码真实内存占用估算并发数（JPX 19 字节/像素、CMYK 7 字节/像素）
- 字体子集化 CFF/Type1C 路线调研结论：现有依赖 typst `subsetter` 0.2 即支持 CFF 轮廓并转 CID 键控，无需新依赖、无许可障碍（详见开发文档备忘录；实现进 0.6.0 尾部或 0.7.0）

### 变更

- 分析器"不支持的编解码"通知文案更新（CCITT/JPX 的可解码形状不再点名）
- 质量门禁基线解码器覆盖 JPX 输入流（`PDF_COMPRESSOR_QUALITY_CORPUS` 语料中的 JPEG 2000 文件纳入 PSNR 基线）
- 测试新增 JPX 集成路径：J2K/JP2 转码一致性（逐字节）、双级 JPX 转 G4、字典不一致与 flate 混合链保持原样、目标大小搜索覆盖；夹具码流 committed 于 `crates/pdf-core/assets/`（`scripts/make-jpx-fixtures.sh` 再生）

## [0.5.0] - 2026-09-05

### 新增

- **带打开密码的 PDF 支持**：GUI、CLI（`--password`）与引擎三个入口都可传入打开密码；未给密码与密码错误分别以明确的错误提示区分（`error.passwordRequired` / `error.wrongPassword`），GUI 在任务失败时弹出密码重试对话框（密码仅存于会话内存，绝不写入持久化队列）
- **CCITT Group 3 输入解码**：老传真/旧扫描仪产的一维 G3（K=0）PDF 图片现可转码压缩（PDF 默认的无 EOL 形态与带 EOL 的 TIFF 风格均支持，含逐行字节对齐）；二维 G3（K>0）仍保持跳过不破坏
- **目标大小模式的逐图质量分配**：搜索轮内按图片内容细节分层调整质量（平坦图降低、细节图提高），在同等体积预算下获得更好的观感；普通压缩模式行为不变
- **质量回归门禁**：PSNR 逐预设下限与单调性测试；`PDF_COMPRESSOR_QUALITY_CORPUS` 环境变量指向真实 PDF 目录时可运行基线快照回归（体积比 +5% 或 PSNR −1dB 告警）
- **大纲/书签保留专项测试**：嵌套书签夹具钉死常规压缩与目标大小搜索两条路径的书签完整性
- **三平台文件管理器右键集成**：
  - GNOME Nautilus / Nemo：Scripts 菜单（5 个动作 + 安装脚本）
  - Windows：NSIS 安装器自动注册资源管理器 PDF 子菜单（安装时写入、卸载时清理）
  - macOS：Finder Quick Action（安装脚本将 CLI 路径烘焙进服务）
  - headless CLI 随 Windows/macOS 桌面安装包一同分发
- **应用内更新检查**：设置页"软件更新"卡片（手动检查、确认后下载安装、重启）；Tauri updater 插件 + 签名公钥内置，私钥经 `TAURI_SIGNING_PRIVATE_KEY` 环境变量注入发布流程

### 修复

- 修复 G3 解码在数据流恰好结束于最后一行时因码表先行位不足而误判失败的问题（解码前在副本尾部补零）

## [0.4.0] - 2026-08-30

### 新增

- **目标大小模式升级为一等压缩模式**：与预设并列出现在主视图、设置页与右键配置中；搜索引擎改为全量程二分（起点为文档原始最大边长，预设值仅作提示不作上限），预算富余时优先提升质量
- **预设体系贯通**：主视图快捷选择、设置页全参数编辑（含自定义预设）、右键快速压缩跟随预设配置；预设持久化为 `preset-user-config.json`
- **独立设置视图**：外观（主题/语言）、压缩参数预设、右键快速压缩参数分卡片管理
- quick 模式配置档案（`quick-profile.json`）：GUI 编辑、headless CLI 读取

### 变更

- 侧栏与设置页布局重构
- 升级 `@tauri-apps/api` 2.11 与 `plugin-dialog` 2.7；`sync-version` 同步全部 workspace crate 版本

### 修复

- GUI 安装包启用 `custom-protocol` 内嵌前端资源，脱离 dev server 可用

## [0.3.0] - 2026-08-28

### 新增

- **CCITT Group 4 黑白编码出口**：近双色调图片无损转 G4（较 JPEG 大幅缩小）；GUI 色彩模式三态（彩色/灰度/黑白 G4）、CLI `--bilevel g4`；扫描件专用压缩管线接入 GUI（scan-heavy 文档自动路由）
- **字体子集化**（opt-in）：内嵌 Type0/CIDFontType2 TrueType 字体裁剪为实际使用的字形，内容流零改写（`/CIDToGIDMap` 桥接，`/W` 数组保持原样）
- **通用流去重**：字节级相同的图片、内容流、字体程序、Form XObject 无损合并，入边引用改写、不留 stub 对象
- **未引用资源清理**：内容树可证明未引用的 `/Font`、`/XObject` 资源条目移除（保守失败策略）
- **ICC / Indexed 色彩空间支持**：ICC 灰度/RGB 与索引调色板图片可安全重编码，重建时保留 ICC profile 引用
- **后台右键压缩模式**（`quick` 子命令）：Dolphin ServiceMenu 五个动作，桌面通知汇报结果
- **加密安全护栏**：owner-only 密码文档自动解锁并明文输出（附通知）；需要真实密码的文档在入口处拒绝，绝不产出损坏文件
- 压缩结果报告、单文件压缩、会话队列恢复（含缺失文件预检）
- JBIG2 许可证门禁评估结论：生态候选均有 GPL/AGPL 传染风险，暂不引入（详见开发文档）

### 变更

- **目标大小搜索改为内存探测**：文档只准备一次，解码位图跨轮缓存，探测全程不落盘
- 灰度重编码（RGB→Luma）作为一等设置
- JPEG 编码器切换为 `jpeg-encoder`（SIMD，实测约 3 倍提速，输出更小）
- MSRV 提升至 1.88（引擎），升级 jpeg-encoder 0.7

## [0.2.0] - 2026-04-16

### 新增

- Fluent Design 风格 UI 与桌面工作流（多文件队列、逐文件设置、进度与取消）
- 类型化 IPC（tauri-specta 生成 TypeScript 绑定）
- AUR 打包与 Linux 安装文档

### 变更

- 引擎抽取为独立 crate（`pdf-core`），CLI、基准与模糊测试共享同一引擎
- 压缩引擎优化与模块拆分

## [0.1.0] - 2026-03-23

### 新增

- 首个版本：纯 Rust 分析引擎（文本/扫描件分类、预估与预设推荐）、选择性压缩管线（JPEG 重编码 + 下采样）、Tauri 桌面壳

[未发布]: https://github.com/cosct/pdf-compressor/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/cosct/pdf-compressor/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/cosct/pdf-compressor/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/cosct/pdf-compressor/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/cosct/pdf-compressor/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cosct/pdf-compressor/releases/tag/v0.1.0
