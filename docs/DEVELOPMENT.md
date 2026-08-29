# 开发指南 / Development Guide

本文面向贡献者，覆盖环境搭建、架构总览、测试体系、代码规范与构建打包。
For contributors: setup, architecture, testing, conventions, and packaging.

## 1. 环境要求

| 依赖 | 说明 |
| --- | --- |
| Node.js 22+ / npm | 前端工具链（Vite、Vitest、vue-tsc） |
| Rust 工具链 | MSRV：`crates/pdf-core` 为 1.88，`src-tauri` 为 1.93（CI 强制校验） |
| Tauri 系统依赖 | Linux 需要 `webkit2gtk-4.1`、`gtk3`、`librsvg`、`patchelf`；其他平台见 Tauri 官方文档 |
| cargo-fuzz（可选） | 模糊测试，需要 nightly 工具链 |
| cargo-mutants（可选） | 变异测试 |

```bash
# 安装 JS 依赖
npm install

# 安装可选的 Rust 工具（按需）
cargo install cargo-fuzz cargo-mutants
```

## 2. 常用命令速查

```bash
npm run dev                  # 仅启动前端（浏览器预览，无原生能力）
npm run tauri dev            # 完整桌面应用开发模式（推荐日常使用）
npm test                     # 前端单元测试（Vitest）
npm run build                # 前端类型检查（vue-tsc）+ 生产构建

cargo test --workspace       # Rust 单元 + 集成测试 + bindings 再生成
cargo clippy --workspace --all-targets -- -D warnings   # CI 同款 lint
cargo bench -p pdf-core      # 压缩性能基准（criterion）

cargo run -p pdf-core --bin pdf-compressor-cli -- analyze <file.pdf>
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --preset maximum
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --target-size 5MB
cargo run -p pdf-core --bin pdf-compressor-cli -- quick <file.pdf> --grayscale   # 后台模式（右键集成用）
```

## 3. 架构总览

```
┌─────────────────────────────┐       ┌──────────────────────────────┐
│  前端 (src/, Vue 3 + TS)     │  IPC  │  桌面壳 (src-tauri, Tauri 2) │
│  App.vue ─ 主视图/设置视图    │ ◄───► │  commands.rs（12 个命令）     │
│  usePdfCompressor（核心状态）│ typed │  任务注册表 / 输出路径白名单    │
└─────────────────────────────┘  IPC  └──────────────┬───────────────┘
                                                      │ 直接调用
                                        ┌─────────────▼──────────────┐
                                        │ 引擎 (crates/pdf-core)      │
                                        │ analyzer / compressor /     │
                                        │ target_size / settings      │
                                        └────────────────────────────┘
```

### 分层规则

- **`crates/pdf-core`**：纯 PDF 引擎，不依赖 Tauri/UI。公共 API 只有
  `analyze_pdf_with_progress`、`compress_pdf_with_progress`、
  `compress_pdf_to_target_size`、`CompressionSettings(Overrides)` 与错误/模型类型，
  在 `src/lib.rs` 统一导出。四个前端共享它：Tauri 应用、`pdf-compressor-cli`、criterion 基准、cargo-fuzz。
  引擎内部分层：`pdf/compressor.rs`（文档编排与批量调度）、`pdf/encode.rs`（单图
  编解码：跳过启发式/解码/缩放/JPEG 重编码/软蒙版）、`pdf/search.rs`（目标大小搜索
  状态：逐图缓存与探测/物化）、`pdf/target_size.rs`（搜索调度与入口）、
  `pdf/workers.rs`（共享有界作用域线程池，图片优化与探测轮共用）。
- **`src-tauri`**：薄 IPC 壳。命令解析请求、合并设置、注册取消标志、`spawn_blocking`
  调引擎、把输出路径登记进 `SessionOutputRegistry`（`open_path`/`reveal_path_in_folder`
  只接受本会话产物，防止 IPC 沦为“打开任意文件”的原语）。
- **前端**：无 Pinia，状态集中在单例 composable `usePdfCompressor`
  （队列、逐任务设置、并发调度、取消、localStorage 持久化）。
  `src/lib/bindings.ts` 是 tauri-specta 生成的类型化 IPC 层（勿手改）；
  `src/lib/tauri.ts` 在其上封装进度通道归一化与浏览器降级。

### 一次压缩请求的数据流

1. 前端调用生成的 binding → Tauri IPC → `commands::compress_pdf`。
2. 命令解析路径、过滤 `target_size_bytes`、经 `CompressionSettings::from_sources`
   合并设置（优先级：overrides > payload > preset 默认值，并夹紧范围）。
3. 注册 `AtomicBool` 取消标志 → `spawn_blocking` → 按是否设置目标大小路由到
   `compress_pdf_to_target_size`（内存中二分搜索质量/边长）或 `compress_pdf_with_progress`。
4. 进度经 `tauri::ipc::Channel<ProgressUpdate>` 回传；引擎内 `ensure_not_cancelled` 轮询取消。
5. 输出写为 `<原名>__optimized-<preset>.pdf`（重名追加数字后缀），响应登记输出路径。

## 4. 测试体系

| 层 | 命令 | 覆盖内容 |
| --- | --- | --- |
| 前端单元 | `npm test` | 队列/调度/取消（usePdfCompressor）、通知计时（useErrorToasts）、右键菜单键盘可达性、设置面板（预设/校验/应用到全部）、App 装配冒烟、en/zh-CN key 树一致性 |
| 引擎单元 | `cargo test -p pdf-core --lib` | 设置夹紧与优先级、分析推荐公式、JPEG 头解析、worker 数量界、resize 行为、目标大小搜索调度数学（质量二分/边长收缩/边界重置） |
| 引擎集成 | `cargo test -p pdf-core --lib`（`pdf/tests.rs`） | 真实 lopdf 构造的 PDF：往返保文本且缩减>50%、去重、SMask、灰度、目标大小、96 用例变异语料不 panic、真加密拒绝/owner-only 解锁、多图小流解除跳过、灰度强制重编码、不写更大输出、预估排除不可解码编码器 |
| CLI 单元 | `cargo test -p pdf-core --bin pdf-compressor-cli` | `parse_size`/`flag_value`/`split_quick_inputs` 参数解析、quick 通知文案（en/zh） |
| 桌面壳单元 | `cargo test -p app --lib` | 任务注册表（注册/取消/注销）、输出路径白名单（含规范化）、预设配置原子写/读回/清除/损坏 JSON、`existing_paths` 过滤 |
| Bindings | `cargo test --workspace`（含 `export_bindings`） | 由 Rust 签名再生成 `src/lib/bindings.ts` —— **命令签名变更后必须运行并提交再生成结果** |
| 基准 | `cargo bench -p pdf-core` | 全管线各预设、编码器对比（jpeg-encoder vs image crate）；夹具生成器共享自 `testutil` |
| 变异测试 | `cargo mutants`（在 `crates/pdf-core` 下，配置 `.cargo/mutants.toml`） | 每周一 CI 自动跑（`.github/workflows/mutants.yml`），报告在 `mutants.out/` |
| 模糊测试 | `cd crates/pdf-core && cargo +nightly fuzz run pipeline` | 任意字节跑 analyze+compress+目标大小搜索；CI 每次 push 冒烟 60s |

夹具共享：`crates/pdf-core/src/testutil.rs` 提供确定性图片/JPEG 生成器
（`deterministic_rgb_image`/`gradient_rgb_image`/`fixture_rgb_image`/`encode_jpeg`），由 `testutil` feature
控制，经 crate 对自身的 dev-dependency 只在测试/基准/示例中启用（resolver = "2"
保证不泄漏进正常构建）。集成测试、两个 bench 与 `examples/make_fixture.rs` 共用它，
不要在各处复制生成器。

### 引擎加固要点（维护者备忘）

- **lopdf 0.44 是硬要求**：0.38 的 `save_modern` 会把第 2 个及以后的 ObjStm 分配在
  xref `/Index` 枚举上界之外（`create_xref_steam` 以构造时的 `size` 为界），poppler 渲染
  时报 `Invalid XRef entry N`。升级前所有多 ObjStm 输出都带此警告；勿降级。
- **SkipPolicy**（`encode.rs`）：小流跳过阈值是文档级策略——图片对象数 ≥ 24 或显式
  灰度/G4 双级请求时解除（降到 6KB tiny 下限）。分析器的预估经 `image_is_actionable` 镜像同一
  套启发式，两边必须同步改，否则预估重新失真。
- **CCITT Group 4**（`encode.rs`，feature `ccitt` 默认开启）：输出侧按“近双级”判定
  （midtone 占比 ≤ 5%，`NEAR_BILEVEL_MIDTONE_FRACTION`）决定 JPEG 还是 G4，连续调图永远走
  JPEG；输入侧只解码纯 G4 形状（单一 `CCITTFaxDecode` 过滤器 + `/DecodeParms` K<0 + 无
  `EncodedByteAlign`），G3（K≥0）与 flate 混合链保持 skip——判定收口在 `stream_filter_info`
  的 `ccitt_decodable`，分析器自动跟随。目标大小搜索中 G4 无质量旋钮，产物按尺寸 memo 于
  `ImageSearchCache::bilevel_product`。**JBIG2 门禁结论（2026-08，暂不引入）**：
  Rust 生态两个候选均非直接可用——`jbig2enc-rust` 声称 MIT OR Apache-2.0，但默认开启的
  `symboldict` 特性含改编自 djvulibre 的代码（GPL 传染风险）、仓库无独立 LICENSE 文件、
  且是对 AGPL-3.0 的 C 版 jbig2enc 的移植；`jbig2enc`（tagawa0525/jbig2enc-rs，Apache-2.0）
  自称 reimplementation，但算法源自同一 AGPL 原版，衍生关系未经验证。本项目以 MIT 分发
  二进制，在出现可验证清洁来源的实现前保持 G4 唯一双级出口；如需 JBIG2 再按
  T.88 规范自研 generic-region 编码器（无符号字典，收益约 10-25%，约 2-3 周）另立决策。
- **灰度/G4 设置链路**：`grayscale: bool` + `bilevel_codec: BilevelCodec`（"jpeg"/"ccitt-g4"
  字符串上 IPC 线格式）；GUI 的“色彩模式”三态选择在 `CompressionSettingsPanel` 里映射成这对
  字段（黑白 = grayscale+G4）。CLI 为 `--grayscale` / `--bilevel g4`（后者已入
  `split_quick_inputs` 的 `VALUE_FLAGS`）。
- **quick 模式配置档案**（`quick_profile.rs`）：`<os-config-dir>/pdf-compressor/
  quick-profile.json`，GUI 设置视图的“右键快速压缩”区块写入（`load_quick_profile`/
  `save_quick_profile` 命令），CLI `quick` 子命令读取。字段优先级：显式 CLI 参数 >
  配置档案 > 内置默认（`CompressionSettingsOverrides::or_else` 逐字段回落）；档案缺失或
  损坏时 quick 静默回退默认值（stderr 警告），绝不因配置失败。面板里选择预设（含
  自定义预设）会把该预设的参数物化进档案——预设的百分比边长按共享参考边
  `DEFAULT_REFERENCE_IMAGE_EDGE_PX`（3200px）折算成绝对像素，因为 headless 路径没有
  逐文件分析参考。
- **预设档案**（`PresetProfilePayload` / `preset-user-config.json`）：每个预设携带完整
  参数组（质量、最大边长百分比、四个开关、灰度/双级编解码）。旧配置文件只有两个
  字段也能解析——新增字段全为可选，由前端 sanitize 回填内置默认。
- **目标大小搜索**（`target_size.rs`）：经典二分求“适配预算的最高质量”；整个质量范围
  失败才收缩边长，且新边长下 `hi` 重置为触发塌缩的质量（不是用户质量）。best-effort
  兜底取“estimate 最小的探测参数”。**中间探测轮的 materialize 禁止 renumber**——
  `save_and_build_response_with_renumber(…, false)`：renumber 会使搜索条目持有的
  object id 全部失效，后续轮（或最优轮恢复）会在陈旧 id 上插入流，产出内容错乱的文件。
- **加密守卫**（`pdf/mod.rs::ensure_not_encrypted`）：lopdf 加载时空密码解密成功的
  文档（仅 owner 密码）trailer 已无 `/Encrypt`、状态记于 `Document::encryption_state`；
  认证失败的（真用户密码/DRM）对象图未解析、页数为 0。守卫据此放行前者（清状态 +
  双端通知）并拒绝后者。
- **不写更大输出**（`compressor.rs::save_and_build_response_with_renumber`）：先在内存
  序列化再比较原件，未胜出时不落盘、`output_path` 置空（前端据此禁用打开/显示），
  并附 `compress.warning.outputNotSmaller` 通知。
- **通用流去重**（`compressor.rs::dedupe_identical_streams`）：全字典等价 + 内容逐字节
  相等才合并（lopdf `Dictionary` 是 IndexMap 保插入序，指纹/校验都做了序无关处理；
  字典内引用按目标 id 比较——同目标可合并、不同目标不合并，天然覆盖 SMask/ICC 数组）。
  合并走**入边引用改写**后删除重复对象（`dedupe_apply_replacements`），不再留
  “间接对象体内是裸引用”的 stub；该形态虽被多数阅读器容忍，但不符合规范。
- **未引用资源清理**（`resources.rs`）：解析页与 Form 的内容流（`Tf`/`Do` 操作数），
  删除从未被引用的 `/Font`、`/XObject` 条目，孤儿对象由保存时 `prune_objects` 兜底。
  **保守失败**：带 /AP 的注解、/Pattern 非空、Type3 字体、Form 缺自身 /Resources、
  名字在当前字典解析不到（可能依赖继承回退）、内容解码失败——任一命中即整页保留。
  逐页独立 /Resources 与共享 /Resources 对象（按 id 分组取名字并集）都支持。
- **色彩空间解析**（`colorspace.rs`）：`optimize_image_stream` 处理已移出文档的流，
  ICC 的 `/N`、Indexed 查找表、资源字典 `/ColorSpace` 名字别名都必须在
  `prepare_document` 阶段（文档完整时）解析成 `ImageColorSpaceInfo` 并随
  `ImageTask`/`ImageSearchEntry` 携带。别名表取全部资源字典的并集，同名不同值
  视为歧义弃用。重建流时若通道数匹配则**保留原 ICC 数组**（profile 不丢），
  Indexed 输出声明基色空间；灰度/G4 转换导致通道数变化时回退 Device 名。
  CMYK（N=4）、CMYK 基 Indexed、JPX 保持跳过。
- **JPX（JPEG2000）解码未接入**（刻意保持默认构建纯 Rust）：候选路线按优先级——
  ① `jpeg2k` crate（支持链接系统 OpenJPEG，避免 vendored 源码构建）；② 自写
  系统 libopenjp2 的最小 FFI（pkg-config + ~150 行 unsafe，需配 fuzz）；
  ③ `openjpeg-sys`（vendored cmake 构建，最重）。无论哪条都需要 unsafe C 解码
  路径配合 cargo-fuzz 强化后再开 feature `jpx`（默认关）。解码后走既有
  JPEG/G4 重编码管线，DecodeParms/JPXColorSpace 处理可参考 CCITT 的形状门禁。
- **线性化（Fast Web View）暂缓**：lopdf 0.44 的 `SaveOptions::linearize` 是**空壳**——
  `save_with_options` 完全忽略该标志（writer 无任何 hint 表/首页分区逻辑，仅
  `object_stream.rs` 里有个“已是线性化文档”的读取侧判断）。自研需按 PDF 32000
  Annex F 实现 hint 流与对象分区，属多周工程；等 lopdf 上游实现或单独立项。
- **字体子集化**（`fonts.rs`，feature `subset-fonts` 默认开、设置 `subset_fonts`
  默认关）：仅覆盖 Type0→CIDFontType2→FontFile2（TrueType 轮廓）且编码为
  Identity-H/V 的字体。typst subsetter 按字形 id 保留并剥离 cmap，故子集只能作
  CID 字体——**内容流零改写**，新字形编号用生成的 `/CIDToGIDMap` 流桥接（BE u16
  per CID）；**`/W` 数组保持不动**（宽度以 CID 为键，ISO 32000 表 115，CID 未变
  则原数组仍然正确——曾按新 GID 重映射，PDFium/Acrobat 系会查表失败，已修正）。
  字形收集解析 Tf/Tj/TJ/'/" 操作数（当前字体状态跟踪 + Form 递归）；任一 `Tf`
  名字在当前资源字典解析不到（可能是继承回退）、选中 Type3 字体（其字形程序是
  本模块不遍历的内容流）、页面带 /AP 注解外观流、或字体程序被非候选字体（如
  简单 TrueType）共享 → **整轮放弃**。共享同一 FontFile2 的多个候选 Type0 字体
  取字形并集、子集化一次。测试字体 `assets/test-font.ttf` 由 pyftsubset
  生成（77 字形），gid 常量见 `testutil.rs`；勿手改。

CI（`.github/workflows/ci.yml`）在每次 push/PR 执行：前端测试+类型检查+构建、
Rust clippy `-D warnings` + 测试、两个 MSRV 检查、60s 模糊测试、依赖审计
（cargo audit + npm audit）。

### 测试约定

- 前端测试在 `src/**/__tests__/*.spec.ts`，`lib/tauri` 一律 mock，只测状态转换。
- 引擎集成测试用 `tests.rs` 顶部的 fixture 生成器构造 PDF，不依赖外部文件。
- 新增 Tauri 命令：改 `commands.rs` → `cargo test --workspace` 再生成 bindings → 前端经
  `src/lib/tauri.ts` 封装调用，禁止组件直接 `invoke`。

## 5. 代码规范与约定

- **Rust**：workspace 级 clippy（`Cargo.toml [workspace.lints]`），CI 以 `-D warnings` 执行；
  `redundant_clone`/`too_many_arguments` 等为 deny。提交前本地跑一遍同款命令。
- **TypeScript/Vue**：`npm run build` 内含 vue-tsc 类型检查；组件内 props down / emits up，
  业务状态只进 `usePdfCompressor`。
- **i18n**：所有用户可见文案进 `src/locales/{en,zh-CN}.ts`，两份文件 key 必须一致；
  后端文案用 code（如 `compress.warning.targetSizeMissed`）+ values 传参，前端
  `backendMessages.ts` 负责本地化与回退（en 缺失时回退 backend `fallback` 文本）。
- **注释**：模块头双语（英/中）说明职责，函数注释只写“为什么”。
- **版本**：`package.json` 是唯一版本源，改版本后运行 `npm run sync-version`
  同步到 `tauri.conf.json` 与 `src-tauri/Cargo.toml`。

## 6. 构建与打包

```bash
npm run build        # 前端生产包（dist/）
npm run tauri build  # 桌面安装包（Windows: NSIS）
npm run tauri:build  # 安装包 + 便携版可执行文件（scripts/postbuild-portable.mjs）
```

- Linux（Arch）：AUR 源码包在 `aur/pdf-compressor/`，PKGBUILD 走
  `npm ci && npm run build` + `cargo build --release --locked`。
- Release 流程见 `.github/workflows/release.yml`；产物命名与标识符见 README「Release metadata」。

## 7. 开发常见问题

- **浏览器预览里 Browse 按钮不可用 / 拖拽拿不到路径**：预期行为。原生能力检测用
  `hasNativeCommands()`（内部 `isTauri()`），浏览器模式自动降级；要验证原生交互请用
  `npm run tauri dev`。
- **改了命令签名后前端类型报错**：运行 `cargo test --workspace` 再生成 bindings 并提交。
- **压缩后文件没变小**：先看分析结果 `documentKind`（text-native 压缩空间有限）、
  是否无内嵌图片、或图片过滤器不受支持（JPX/JBIG2/CCITT/Crypt 会跳过）。
- **目标大小模式未达标**：引擎最多尝试 12 轮（`MAX_ATTEMPTS`），产出“当前可达的最小结果”并返回
  `compress.warning.targetSizeMissed` 提示；前端以警告 toast + 状态卡 notes 呈现。
- **`cargo bench` 名字冲突**：基准每轮使用独立临时目录，避免 100 次重名上限。
