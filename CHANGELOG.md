# 更新日志 / Changelog

本项目的所有显著变更都记录在此文件中。格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。日期为提交日期。

## [0.7.0] - 2026-09-06

### 新增

- **CMYK→RGB 真转换（0.6.0 路线 P0 落地，feature `cmyk-cms`）**：开启 `cmyk_conversion` 后的转换不再用朴素减色公式，而是与主流渲染器逐字节对齐——带 ICC profile 的图片（ICC N=4）经 Little CMS（vendored lcms2 静态编译，同 jpx 模式 feature 门控、默认关）按**嵌入 profile** 转换，且与 poppler 为同一文件构建的 transform 完全同构（Relative Colorimetric + 黑点补偿 → 内建 sRGB）；无 profile 的 CMYK（DeviceCMYK / 4 分量 JPX / CMYK 基 Indexed / 无效 profile）走 poppler 与 PDFium 共用的 xpdf SWOP 16 项矩阵（含其精确舍入）。DCT 路径经 zune-jpeg 取原始 4 分量平面再转换（跳过解码器内置的朴素折叠），Adobe YCCK 按 libjpeg 语义还原（极性经 poppler 26.08 实证钉死）。**验收**：CGATS TR 001（CC0）嵌入的 raw 与 Adobe YCCK DCT 双夹具上，压缩前后 poppler 渲染 PSNR 实测 ≈54.6dB（门槛 ≥25dB；0.6.0 朴素基线 ≈7.6dB），渲染比对门禁进 CI 可选特性腿；`cmyk_conversion` 默认仍为关（源码默认构建的朴素路径未达标，翻转评估见开发文档路线 §8），桌面 release 包与 AUR 包启用该 feature
- **Arch Linux 安装包进入 Release 产物**：release 流程现直接产出 `pdf-compressor_<版本>_amd64.pkg.tar.zst`，随 deb/AppImage/NSIS/dmg 一同挂到 GitHub Release——CI 在 archlinux:base-devel 容器里复用 ubuntu 构建的原始二进制，跑与本地 `pnpm run tauri:arch` 完全相同的免编译 makepkg 管线（`scripts/build-arch-bundle.sh`），不重编译、与 bundle 目标零漂移
- **AUR 自动发布**：打 `v*` tag 后 CI 自动把 PKGBUILD（含 Release zst 的真实 sha256）与再生成的 `.SRCINFO` 推送到 AUR；需在仓库 secret 配置 `AUR_SSH_PRIVATE_KEY`（公钥注册到 AUR 账号），未配置时跳过推送、其余产物照常发布

### 变更

- **fuzz 强化**：pipeline 模糊目标开启 `cmyk_conversion` 并以 `cmyk-cms` 特性构建——任意 ICC profile 字节随变异 PDF 进入 vendored lcms2 解析器（与 jpx 的 C 面同等级暴露）
- 4 分量 JPX 的 CMYK 转换随 chokepoint 切换（cms 构建下为 SWOP 矩阵，朴素构型行为不变）
- **AUR 分发从源码包切换为二进制包**：包名 `pdf-compressor` → `pdf-compressor-bin`（声明 `provides`/`conflicts` 旧名，安装即替换），安装不再需要 Rust/Node 工具链与全量编译；原源码包停止更新
- 适配 clippy 1.98 新 lint（`chunks_exact` → `as_chunks` 等）；修复 MSRV CI 作业在 src-tauri 缺 `frontend/dist` 占位时的构建失败

> 0.6.0 路线中的 P2 搭车项（JPX/DCT 编码的 `/SMask` 解码、JPX 子采样分量上采样）未随 0.7.0 落地，顺延评估。

## [0.6.0] - 2026-09-06

### 修复（2026-09 外部审查，8 项）

- **[P1] 共享 Form 资源误删**：两个 Form 共享 `/Resources` 对象时清理互相删除对方字体，两段文字消失——使用集改为按资源字典归宿归并（共享对象取并集后一次清理），未被遍历页引用的 Form 共享资源加入否决
- **[P1] CCITT G4 输出黑白反转**：引擎输出的 G4 位流在渲染器（poppler 实测）下白底扫描件变黑底白字。根因是 fax crate 的 Color 标签与 T.4 码表绑定交叉（编/解码两侧对称互反，往返测试自洽掩盖了问题）。统一修正极性矩阵（G4 编码翻转喂色、G4 transitions 解码翻转极性、G3 路径按码表直读），新增 tiffcp 参考位流逐像素比对 + 独立渲染门禁；CCITT 输入携带 `/Decode` 数组保持跳过
- **[P1] 透明蒙版 `/Decode` 丢弃**：`/Decode [1 0]` 的 SMask 透明度左右反转——读取时归一化采样值（重建蒙版无 Decode 语义不变），非单位/反转之外的映射安全跳过
- **[P1] 同名并发导出互相覆盖**：`exists()` 检查存在竞态，两个目录的 report.pdf 同时导出只剩一个结果——输出名改为 `create_new` 原子占用 + 占位守卫（失败/取消清理零字节占位）
- **[P1] 队列满时取消永久阻塞**：调度线程阻塞在满通道的 `send()`，取消后线程池不返回——投递改 `try_send` 轮询并响应取消
- **[P1] 字体子集化漏收 q/Q 恢复的字体**：图形状态栈未跟踪，`q…Q` 内临时换字后恢复的 Type0 字体字形漏收（缺字）——收集器维护字体状态栈
- **[P2] 4 位 Indexed 奇数宽行错位**：行末填充半字节被当像素，后续行逐行偏移——按行宽逐行解包
- **[P2] 错误密码后 GUI 无法重试**：分析入口把 WrongPassword 包装成 PdfBuild，密码重试入口失联——密码/加密类错误原样透传

### 新增

- **JBIG2（T.88）输入解码**：扫描文本页的最后一个输入编解码盲区。`hayro-jbig2`（纯 Rust、Apache-2.0 OR MIT、T.88 全量、hayro PDF 渲染器同源）接入，无 feature 门控、默认构建即含。`/JBIG2Globals` 共享符号字典在准备阶段预解析、随流携带到 worker；形状门禁照 CCITT/JPX 模式收口；解码平面（本地 push-based sink 直组灰度）自动落 G4/JPEG 出口。夹具为 conformance 语料的真实 A4 扫描页（`scripts/make-jbig2-fixture.sh` 再生），poppler 渲染门禁做保真锚点；无收益转码（符号压缩的 JBIG2 常比 G4 更小）被"不写更大输出"规则正确拒绝。输出侧编码维持许可门禁结论（G4 仍是唯一双级出口）
- **CMYK 转换开关（`cmyk_conversion`，默认关）**：实测朴素减色公式与渲染器 CMS 解释存在 ≈7.6dB 系统偏差（质量档不敏感，见 0.7.0 调研）后，CMYK 图片默认回到保持原样的保守行为；开关（GUI 设置卡 / CLI `--convert-cmyk` / quick 档案 / 预设档案）显式开启转换；**灰度/G4 请求隐式开启**（彩色→灰度折叠本身即有损意图，偏差远小）；统一 gate 拦截 ICC N=4 / DeviceCMYK / CMYK 基 Indexed / CMYK JPEG / 4 分量 JPX 全部路径；分析器预估按默认设置镜像（宁低勿高）。0.7.0 将以 lcms2 真转换达标（验收 ≥25dB）后评估翻转默认值
- **JPX（JPEG 2000）输入解码**（feature `jpx`，默认关闭以保持默认构建纯 Rust；桌面 release 安装包与 AUR 包开启）：PDF 内嵌的 JPEG 2000 图片（J2K 裸码流与 JP2 容器两种形态）现可解码转码为 JPEG/G4 压缩；1/3/4 分量分别按灰度/RGB/CMYK 处理。实现经 `jpeg2k`（OpenJPEG 安全封装）——vendored OpenJPEG 用 `cc` 静态编译链接，**无需 cmake、无需在安装包携带运行时动态库**，三平台打包风险由此消除（2026-09 spike 结论）。unsafe C 面配专职 fuzz target（`cargo +nightly fuzz run jpx`，CI 45s 冒烟）；形状门禁照 CCITT 模式在 `stream_filter_info` 收口，分析器预估自动跟随
- **2GiB 输入上限的友好提示**：专用错误码 `error.inputTooLarge`（GUI 双语文案就位），携带人性化体积（如 "2.1 GB"）而非原始字节数
- **内存护栏**：目标大小搜索的缓存预算核算扩展到 alpha/G4 产物（原先只算彩色平面），超限两级淘汰；多图大文档搜索的进程内存峰值测试护栏（Linux，实测 ≈390MB / 上限 2GB）；worker 池按编解码真实内存占用估算并发数（JPX 19 字节/像素、CMYK 7 字节/像素）
- **CFF 字体子集化**：Type0→CIDFontType0（CFF 轮廓，现代 PDF 内嵌字体主流）现可裁剪——FontFile3 的 `/CIDFontType0C`、误标 `/Type1C` 与 `/OpenType` 包装输入均支持。内容流 CID 零改写：subsetter 输出的恒等 charset 经"尾部追加 format-0 charset + 改 Top DICT 偏移"桥接回原 CID；`/W` 保持不动。含最小 CFF 解析器（`pdf/cff.rs`，fail-closed）、poppler 渲染比对门禁（工具缺失时跳过）、fuzz 目标开启字体路径。TrueType（CIDFontType2）子集化行为不变
- 字体子集化 CFF/Type1C 路线调研结论：现有依赖 typst `subsetter` 0.2 即支持 CFF 轮廓并转 CID 键控，无需新依赖、无许可障碍（详见开发文档备忘录；本次据此实现）

### 变更

- **CMYK 默认行为回退**：0.6.0 开发中途引入的 CMYK→RGB 自动转换改为默认关闭（实测偏色后回退到 0.5 的保守行为），需显式开启 `cmyk_conversion`（灰度/G4 模式不受影响，照常转换）
- 分析器"不支持的编解码"通知文案更新（JBIG2/CCITT/JPX 的可解码形状不再点名；JBIG2 已支持解码）
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
