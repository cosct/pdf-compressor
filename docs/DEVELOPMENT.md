# 开发指南 / Development Guide

本文面向贡献者，覆盖环境搭建、架构总览、代码规范与构建打包。
For contributors: setup, architecture, testing, conventions, and packaging.

> 文档地图：用户功能与用法见 [用户指南](USER-GUIDE.zh-CN.md)；测试体系与质量门禁见
> [测试指南](TESTING.md)；版本历史见 [更新日志](../CHANGELOG.md)。

## 1. 环境要求

| 依赖 | 说明 |
| --- | --- |
| Node.js 22+ / pnpm / Vite+ | 前端统一工具链（`vp` 管 dev/build/test/lint/fmt，vue-tsc 负责 .vue 类型检查） |
| Rust 工具链 | MSRV：`crates/pdf-core` 为 1.88，`src-tauri` 为 1.93（CI 强制校验） |
| Tauri 系统依赖 | Linux 需要 `webkit2gtk-4.1`、`gtk3`、`librsvg`、`patchelf`；其他平台见 Tauri 官方文档 |
| cargo-fuzz（可选） | 模糊测试，需要 nightly 工具链 |
| cargo-mutants（可选） | 变异测试 |

```bash
# 安装 JS 依赖
pnpm install

# 安装可选的 Rust 工具（按需）
cargo install cargo-fuzz cargo-mutants
```

## 2. 常用命令速查

```bash
pnpm run dev                  # 仅启动前端（浏览器预览，无原生能力）
pnpm run tauri dev            # 完整桌面应用开发模式（推荐日常使用）
pnpm run check                # 前端格式化 + lint + 类型检查（oxfmt/oxlint/tsgo）
pnpm test                     # 前端单元测试（vp test，Vitest 引擎）
pnpm run build                # 前端类型检查（vue-tsc）+ 生产构建（vp build）

cargo test --workspace       # Rust 单元 + 集成测试 + bindings 再生成
cargo clippy --workspace --all-targets -- -D warnings   # CI 同款 lint
cargo bench -p pdf-core      # 压缩性能基准（criterion）

cargo run -p pdf-core --bin pdf-compressor-cli -- analyze <file.pdf>
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --preset maximum
cargo run -p pdf-core --bin pdf-compressor-cli -- compress <file.pdf> --target-size 5MB
cargo run -p pdf-core --bin pdf-compressor-cli -- quick <file.pdf> --grayscale   # 后台模式（右键集成用）
```

> 前端代码在 `frontend/` 子目录（`index.html` / `src` / `public` / `vite.config.ts` / `tsconfig*`）。
> pnpm 脚本经 `vp -C frontend` 以该目录为根运行；依赖与脚本仍由根 `package.json` 统一管理。

## 3. 架构总览

```
┌─────────────────────────────┐       ┌──────────────────────────────┐
│  前端 (frontend/, Vue 3 + TS) │  IPC  │  桌面壳 (src-tauri, Tauri 2) │
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
  `frontend/src/lib/bindings.ts` 是 tauri-specta 生成的类型化 IPC 层（勿手改）；
  `frontend/src/lib/tauri.ts` 在其上封装进度通道归一化与浏览器降级。

### 一次压缩请求的数据流

1. 前端调用生成的 binding → Tauri IPC → `commands::compress_pdf`。
2. 命令解析路径、过滤 `target_size_bytes`、经 `CompressionSettings::from_sources`
   合并设置（优先级：overrides > payload > preset 默认值，并夹紧范围）。
3. 注册 `AtomicBool` 取消标志 → `spawn_blocking` → 按是否设置目标大小路由到
   `compress_pdf_to_target_size`（内存中二分搜索质量/边长）或 `compress_pdf_with_progress`。
4. 进度经 `tauri::ipc::Channel<ProgressUpdate>` 回传；引擎内 `ensure_not_cancelled` 轮询取消。
5. 输出写为 `<原名>__optimized-<preset>.pdf`（重名追加数字后缀），响应登记输出路径。

## 4. 测试体系

完整的测试金字塔、质量门禁（PSNR 下限/真实语料快照）、模糊与变异测试的运行方式和新测试约定，见 **[测试指南](TESTING.md)**。速查：

| 层 | 命令 |
| --- | --- |
| 前端单元 + lint/类型 | `pnpm test` / `pnpm run check` |
| 引擎单元 + 集成 + CLI + 桌面壳 + bindings 再生成 | `cargo test --workspace` |
| 基准 | `cargo bench -p pdf-core` |
| 模糊测试 | `cd crates/pdf-core && cargo +nightly fuzz run pipeline`（JPX 解码路径另有 `jpx` target） |
| JPX feature 配置的测试/clippy | `cargo test -p pdf-core --features jpx` / `cargo clippy -p pdf-core --features jpx -- -D warnings`（CI 有独立步骤） |
| 变异测试 | `cd crates/pdf-core && cargo mutants` |
| PSNR 质量门禁 | `cargo test -p pdf-core --lib preset_quality` |
| 真实语料快照 | `PDF_COMPRESSOR_QUALITY_CORPUS=<dir> cargo test -p pdf-core --lib real_corpus` |

夹具共享：`crates/pdf-core/src/testutil.rs` 提供确定性图片/JPEG/CCITT 生成器
（`deterministic_rgb_image`/`gradient_rgb_image`/`fixture_rgb_image`/`encode_jpeg`/
`encode_ccitt_g4`/`encode_ccitt_g3_1d`/`luma_psnr_db`），由 `testutil` feature
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
- **CCITT 输入解码**（`encode.rs`，feature `ccitt` 默认开启）：输入侧经 `ccitt_input_shape`
  收口两类可解码形状——纯 G4（K<0、无 `EncodedByteAlign`）与一维 G3（K=0）。G3 细分两路：
  `EndOfLine=true` 走 fax 自带 `decode_g3`（fill/EOL/RTC aware，要求 `!EncodedByteAlign`）；
  PDF 默认的无 EOL 形态走本地 1D MH 读码器（`decode_ccitt_g3_plain`，用 fax 公开的
  `maps::white/black` 码表 + `BitReader`，支持 `EncodedByteAlign` 逐行对齐）。**尾部先行位坑**：
  fax 的码表查找需要越过最后一个码的若干先行位，流恰好结束在最后一行时必须先在副本尾部补
  4 个零字节再解（零序列不是合法码前缀，截断流仍干净失败）。G3 二维（K>0）、flate 混合链、
  EOL+对齐组合保持 skip——判定收口在 `stream_filter_info` 的 `ccitt_decodable`，分析器自动跟随。
  输出侧按“近双级”判定（midtone 占比 ≤ 5%，`NEAR_BILEVEL_MIDTONE_FRACTION`）决定 JPEG 还是
  G4，连续调图永远走 JPEG。目标大小搜索中 G4 无质量旋钮，产物按尺寸 memo 于
  `ImageSearchCache::bilevel_product`。**JBIG2 门禁结论（2026-08，暂不引入）**：
  Rust 生态两个候选均非直接可用——`jbig2enc-rust` 声称 MIT OR Apache-2.0，但默认开启的
  `symboldict` 特性含改编自 djvulibre 的代码（GPL 传染风险）、仓库无独立 LICENSE 文件、
  且是对 AGPL-3.0 的 C 版 jbig2enc 的移植；`jbig2enc`（tagawa0525/jbig2enc-rs，Apache-2.0）
  自称 reimplementation，但算法源自同一 AGPL 原版，衍生关系未经验证。本项目以 MIT 分发
  二进制，在出现可验证清洁来源的实现前保持 G4 唯一双级出口；如需 JBIG2 再按
  T.88 规范自研 generic-region 编码器（无符号字典，收益约 10-25%，约 2-3 周）另立决策。
- **目标大小搜索的逐图质量分配**（`encode.rs`）：搜索轮内（`search_cache.is_some()`）按解码
  后平面的细节分层给质量偏移——平面细节分（`plane_detail_score`，按 4 像素步长采样的平均
  luma 梯度）< 8 视为平坦（q−12），> 25 视为高细节（q+6），夹在 [10,100]。平坦内容低质量
  几乎无感且省字节，预算花在细节图上；非搜索模式严格用用户设定的 q。偏移是（流,边长,灰度）
  的确定性纯函数，探测估计与物化编码保持字节一致（`target_size_search_produces_reproducible_output`
  钉死这一点）。
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
- **加密守卫与密码支持**（`pdf/mod.rs`）：三个入口都接受 `password: Option<&str>`
  （`load_document` 走 lopdf 的 `load_with_password`）。空密码自动解锁的 owner-only 文档
  放行并通知（输出为明文）；未给密码的用户密码文档报 `PasswordRequired`
  （`error.passwordRequired`），给了但错误报 `WrongPassword`（`error.wrongPassword`——lopdf
  加载期直接 Err `InvalidPassword`，在 `load_document` 映射）。GUI 在选中任务的错误为这两
  个码时弹 `PasswordPromptDialog`，提交后带密码重跑分析（密码仅会话内存，永不进持久化队
  列）；CLI 为 `--password`（quick 模式一个密码作用于整批，注意 shell 历史可能记录）。
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
- **CMYK（0.6.0 起 opt-in）**：ICC N=4、`DeviceCMYK` 名、CMYK 基 Indexed 统一经
  `DecodeColorSpace::Cmyk` 解析，解码时用共享的油墨减色转换（`colorspace::
  cmyk_to_rgb`，`(1-cmy)×(1-k)`，u16 定点舍入）转成 RGB 平面后走既有 JPEG 管线。
  重建流声明 `DeviceRGB`——**CMYK 的 ICC/profile 永不随行**（通道数已不符，
  `rebuild_color_space` 对 Cmyk 恒 None，Indexed 的 CMYK 基同理）。带 `/Decode`
  映射数组的 CMYK（名字路径与解析路径都查）保持 skip：映射会重释采样值，朴素
  转换会静默偏色。PDF 原始/JPX 的 CMYK 采样不反转（0=无墨），与 DCT 流内
  Adobe 约定的反相 CMYK 不同——后者由 JPEG 解码器（zune-jpeg）先归一。
  **色彩保真与 `cmyk_conversion` 开关（2026-09 实测后落定）**：朴素减色公式与
  主流渲染器的 CMS 解释存在系统性偏差——以 poppler 为基准，规范 Adobe YCCK
  夹具在 conservative 档下压缩前后渲染差 ≈7.6 dB（质量档不敏感 → 模型差异而
  非压缩损失；暗部与饱和色偏差最大）。因此 CMYK 转换是 **opt-in**（设置
  `cmyk_conversion`，CLI `--convert-cmyk`，GUI 设置卡开关，默认关 = 回到 0.5
  的跳过行为）；**灰度/G4 请求隐式开启**（`settings.converts_cmyk()`）——任何
  彩色→灰度折叠本身就有损意图，CMYK→灰度偏差远小于彩色偏差。gate 在
  `optimize_image_stream` 统一拦截 raw/Indexed/DCT/JPX 四条路径
  （`declares_cmyk`），分析器镜像把 `/DeviceCMYK` 名的图片从预估中剔除
  （按默认关设置镜像，宁低勿高）。`CMYK_TRANSCODE_PSNR_FLOOR_DB`（≈46dB）只钉
  "转换自洽性"，不能证明与渲染器一致；真保真（lcms2 CMS）见 0.7.0 路线。
- **JPX（JPEG 2000）解码**（`jpx.rs`，feature `jpx` **默认关**，保持默认构建纯
  Rust；桌面 release 与 AUR 包开启，src-tauri 经 `--features jpx` 转发）：走
  `jpeg2k` crate（0.10，MIT/Apache）→ `openjpeg-sys` 1.0.x（BSD-2）——**vendored
  OpenJPEG 用纯 `cc` crate 编译并静态链接**，无 cmake、无运行时 libopenjp2 需要随
  安装包分发（2026-09 spike 结论；workspace 的 flate2/zlib-ng 本就依赖 cc，构建
  前提不变）。三路线评估中的“链接系统库”因此弃用——静态链接彻底消解了
  Windows/macOS 打包风险。门禁收口照 CCITT 模式：`jpx_input_shape` 只看流字典
  （尺寸 1..=65535、无 DecodeParms/Decode、单元素过滤链），分析器经
  `stream_filter_info::jpx_decodable` 自动跟随；解码侧再校验码流尺寸与字典一致、
  分量无子采样/无 alpha/精度 ≤16。分量数决定色彩：1→灰、3→RGB、4→CMYK（走
  上面的减色转换）；码流自带的 MCT 已被 OpenJPEG 逆变换，按 `opj_decompress`
  的口径对待。`/DecodeParms`（规范未定义）与 `/Decode` 数组保持 skip。
  **fuzz 强化**：`fuzz_targets/jpx.rs` 把任意字节包成最小合法 PDF 的 JPXDecode
  流并填充到小流跳过阈值之上，确保每次迭代都真正进入 C 解码（通用 pipeline
  target 无法从随机字节演化出带 JPX 流的合法 PDF）；CI 各跑 45s。夹具码流
  committed 在 `assets/jpx-*.j2k/.jp2`（`scripts/make-jpx-fixtures.sh` 再生，
  lossless，与 testutil 的参考平面逐像素一致）。worker 池内存估计对 JPX 按
  19 字节/像素（OpenJPEG 每分量 4 字节采样 + 组装平面）计。JPX 的 4 分量
  （CMYK）解码受 `converts_cmyk()` 门控（`decode_jpx_stream` 的 `allow_cmyk`
  参数），关时不解、整图保持。
- **JBIG2（T.88）输入解码**（`jbig2.rs`，**纯 Rust 无 feature 门控**——
  `hayro-jbig2` 0.3，Apache-2.0 OR MIT，hayro PDF 渲染器同源，T.88/T.30 全
  量，edition 2024 → rustc 1.85+，本仓库 MSRV 1.88 兼容）：扫描文本页的最后
  一个输入编解码盲区。门禁照 CCITT/JPX 模式收口于 `stream_filter_info`
  （单元素链、尺寸 1..=65535、无 DecodeParms/Decode）；`/JBIG2Globals`
  共享符号字典段在 `prepare_document` 阶段（文档完整时）取出 bytes，经
  `ImageTask`/`ImageSearchEntry` 随流携带到 worker（与 ICC 色彩空间同款的
  预解析模式），`Image::new_embedded(data, globals)` 解码。解码走本地
  push-based sink（`PlaneSink`）直组 8 位灰度平面（黑=0，与 G4 出口同极性），
  近双级平面自动落 G4/JPEG 出口。**注意**：符号压缩的 JBIG2 文本页常比 G4
  重编码更小——"不写更大输出"规则会正确拒绝无收益转码（集成测试因此用
  target-size 模式物化输出做渲染验证）。夹具：conformance 语料的真实 A4 扫描
  页（sequential 组织）剥去独立文件头转 embedded 形态，committed 于
  `assets/jbig2-scan.bin`（`scripts/make-jbig2-fixture.sh` 再生并打印页尺寸）；
  无许可可用的编码器可派生独立像素真值，保真锚点是 poppler 渲染对比门禁。
  **输出侧维持 JBIG2 门禁结论（2026-08 备忘录）**：G4 仍是唯一双级出口。
- **线性化（Fast Web View）暂缓**：lopdf 0.44 的 `SaveOptions::linearize` 是**空壳**——
  `save_with_options` 完全忽略该标志（writer 无任何 hint 表/首页分区逻辑，仅
  `object_stream.rs` 里有个“已是线性化文档”的读取侧判断）。自研需按 PDF 32000
  Annex F 实现 hint 流与对象分区，属多周工程；等 lopdf 上游实现或单独立项。
- **字体子集化**（`fonts.rs`，feature `subset-fonts` 默认开、设置 `subset_fonts`
  默认关）：覆盖 Type0→CIDFontType2→FontFile2（TrueType 轮廓）与
  Type0→CIDFontType0→FontFile3（CFF 轮廓：`/CIDFontType0C`、被误标的
  `/Type1C`、`/OpenType` 包装）且编码为 Identity-H/V 的字体。typst
  subsetter 按字形 id 保留并剥离 cmap，故子集只能作 CID 字体——**内容流零改写**；
  **`/W` 数组保持不动**（宽度以 CID 为键，ISO 32000 表 115，CID 未变则原数组
  仍然正确——曾按新 GID 重映射，PDFium/Acrobat 系会查表失败，已修正）。
  TrueType 侧用生成的 `/CIDToGIDMap` 流桥接新字形编号（BE u16 per CID）；
  **CFF 侧没有这个键——字体自身的 charset 就是 CID→字形映射**，而 subsetter
  重建 charset 时用恒等映射且不保留原 CID，故走 `cff.rs` 的桥接：解析原
  charset 得到 cid↔gid 双向表（重复 CID、解析不出的 CID、SID 键控程序都整个
  放弃）→ 裸 CFF 包一层最小 OTTO 喂给 subsetter → 从输出的 OpenType 里抽回
  `CFF ` 表 → 尾部追加 format-0 charset 把每个保留字形的原 CID 还给它 →
  改写 Top DICT 的 charset 偏移（subsetter 固定写 5 字节整型操作数，改偏移
  不引起任何位移）→ 以 `/CIDFontType0C` 重嵌。注：subsetter 重写的 ROS 恒为
  Adobe-Identity-0，字形查找走 charset 不走 ROS，PDF 的 CIDSystemInfo 保持
  原样（Identity ordering 的字体天然一致；Adobe-Japan1 等非 Identity ordering
  的 ROS 会变成元数据失配，主流渲染器不据此查字形，属已知取舍）。
  字形收集解析 Tf/Tj/TJ/'/" 操作数（当前字体状态跟踪 + Form 递归）；任一 `Tf`
  名字在当前资源字典解析不到（可能是继承回退）、选中 Type3 字体（其字形程序是
  本模块不遍历的内容流）、页面带 /AP 注解外观流、或字体程序被非候选字体（如
  简单 TrueType/Type1）共享（消费映射 FontFile2+FontFile3 都算）→ **整轮放弃**。
  共享同一字体程序的多个候选 Type0 字体取字形并集、子集化一次。测试字体：
  `assets/test-font.ttf`（pyftsubset 生成，77 字形，gid 常量见 `testutil.rs`）
  与 `assets/test-font-cid.cff/.otf`（Source Han Serif CN 的 CID 键控子集，
  40 字形、刻意非恒等 charset，`scripts/make-cff-fixture.sh` 再生——改字形集
  必须连同 `TEST_CFF_CID_*` 常量一起改）；勿手改。CFF 路径有 poppler
  （pdftoppm）渲染比对门禁兜底（工具缺失时跳过）。老 PFB Type1（FontFile）
  与简单字体 + Type1C（单字节编码经 /Encoding 差异映射，subsetter 转 CID 键控
  后单字节码不再可用）不支持，保持跳过。
  （2026-09 调研备注：subsetter 0.2 即支持 CFF 轮廓并把 SID 键控转 CID 键控，
  无需新依赖、无 JBIG2 式许可障碍，据此把 CFF 子集化提进 0.6.0 实现。）

CI（`.github/workflows/ci.yml`）在每次 push/PR 执行：前端测试+类型检查+构建、
Rust clippy `-D warnings` + 测试、两个 MSRV 检查、60s 模糊测试、依赖审计
（cargo audit + pnpm audit）。

### 测试约定

- 前端测试在 `frontend/src/**/__tests__/*.spec.ts`，`lib/tauri` 一律 mock，只测状态转换。
- 引擎集成测试用 `tests.rs` 顶部的 fixture 生成器构造 PDF，不依赖外部文件。
- 新增 Tauri 命令：改 `commands.rs` → `cargo test --workspace` 再生成 bindings → 前端经
  `frontend/src/lib/tauri.ts` 封装调用，禁止组件直接 `invoke`。

## 5. 代码规范与约定

- **Rust**：workspace 级 clippy（`Cargo.toml [workspace.lints]`），CI 以 `-D warnings` 执行；
  `redundant_clone`/`too_many_arguments` 等为 deny。提交前本地跑一遍同款命令。
- **TypeScript/Vue**：`pnpm run build` 内含 vue-tsc 类型检查；组件内 props down / emits up，
  业务状态只进 `usePdfCompressor`。
- **i18n**：所有用户可见文案进 `frontend/src/locales/{en,zh-CN}.ts`，两份文件 key 必须一致；
  后端文案用 code（如 `compress.warning.targetSizeMissed`）+ values 传参，前端
  `backendMessages.ts` 负责本地化与回退（en 缺失时回退 backend `fallback` 文本）。
- **注释**：模块头双语（英/中）说明职责，函数注释只写“为什么”。
- **版本**：`package.json` 是唯一版本源，改版本后运行 `pnpm run sync-version`
  同步到 `tauri.conf.json` 与 `src-tauri/Cargo.toml`。

## 6. 构建与打包

```bash
pnpm run build        # 前端生产包（frontend/dist/）
pnpm run tauri build  # 桌面安装包（Windows: NSIS）
pnpm run tauri:build  # 安装包 + 便携版可执行文件（scripts/postbuild-portable.mjs）
pnpm run tauri:arch   # 安装包 + Arch zst 包（release/bundle/archlinux/，scripts/build-arch-bundle.sh）
```

- Linux（Arch）：AUR 源码包在 `packaging/archlinux/`，PKGBUILD 走与 deb/appimage
  相同的 `tauri build --no-bundle` 管线（另编 headless CLI），不会与 bundle 目标漂移。
- Release 流程见 `.github/workflows/release.yml`；产物命名与标识符见 README「Release metadata」。

### 文件管理器右键集成（三平台）

| 平台 | 机制 | 位置 |
| --- | --- | --- |
| KDE Dolphin | KIO ServiceMenu（系统级，随包安装） | `packaging/servicemenus/` |
| GNOME Nautilus / Nemo | Scripts 菜单（用户级，`packaging/nautilus/install.sh` 安装；Arch 包把脚本放在 `/usr/share/pdf-compressor/nautilus/`） | `packaging/nautilus/` |
| Windows 资源管理器 | NSIS 安装钩子在安装时写 `SystemFileAssociations\.pdf\shell` 注册表子菜单，卸载时删除；调用随包分发的 CLI | `packaging/windows/context-menu.nsh` |
| macOS 访达 | Quick Action（用户级，`packaging/macos/quick-action/install.sh` 安装到 `~/Library/Services`） | `packaging/macos/quick-action/` |

Windows/macOS 的 CLI 经 `tauri.release*.conf.json` 的 `resources` 随应用分发（release CI
先构建再复制进 `src-tauri/binaries/`；本地 `tauri build` 不要求该文件存在，故资源声明放在
release overlay 而非主配置）。

### 应用内更新（plugin:updater）

- 公钥固化在 `tauri.conf.json` 的 `plugins.updater.pubkey`；**私钥不在仓库**——本机生成于
  `~/.tauri/pdf-compressor-updater.key`（minisign 格式，无口令），**必须备份**，丢失即永远
  无法再签发更新。发布时 CI 从 GitHub secret `TAURI_SIGNING_PRIVATE_KEY`（及可选的
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）读取并签名，`latest.json` 由 tauri-action 附着到
  release。前端入口在设置页 `UpdaterPanel`（手动检查、确认后下载、重启应用）。
- 生成新密钥对：`pnpm exec tauri signer generate -w ~/.tauri/pdf-compressor-updater.key`
  （换钥 = 所有旧版本收不到新更新，慎重）。

## 7. 开发常见问题

- **浏览器预览里 Browse 按钮不可用 / 拖拽拿不到路径**：预期行为。原生能力检测用
  `hasNativeCommands()`（内部 `isTauri()`），浏览器模式自动降级；要验证原生交互请用
  `pnpm run tauri dev`。
- **改了命令签名后前端类型报错**：运行 `cargo test --workspace` 再生成 bindings 并提交。
- **压缩后文件没变小**：先看分析结果 `documentKind`（text-native 压缩空间有限）、
  是否无内嵌图片、或图片不可行动（Crypt 恒跳过；JPX/CCITT/JBIG2 按形状门禁处理，
  JPX 需 feature——默认构建跳过，release/AUR 包已开启；CMYK 默认保持原样，需
  开 `cmyk_conversion` 或灰度模式；符号压缩的 JBIG2 文本页可能本来就比 G4
  重编码更小，"不写更大输出"会正确拒绝）。
- **目标大小模式未达标**：引擎最多尝试 12 轮（`MAX_ATTEMPTS`），产出“当前可达的最小结果”并返回
  `compress.warning.targetSizeMissed` 提示；前端以警告 toast + 状态卡 notes 呈现。
- **`cargo bench` 名字冲突**：基准每轮使用独立临时目录，避免 100 次重名上限。

## 8. 版本路线（0.7.0 评估，2026-09；P0 开关与 P1 已随 0.6.0 落地）

0.6.0 收口范围：JPX 解码、CMYK 三路径（opt-in 开关）、搜索缓存淘汰与内存护栏、
2GiB 友好报错、CFF/Type1C 字体子集化、审查修复 8 项、JBIG2 输入解码。

### P0：CMYK 色彩保真（0.6.0 已落开关，0.7.0 做真转换）

- **实测问题**（见上文 CMYK 段）：朴素减色公式与渲染器 CMS 的系统偏差
  ≈7-9 dB（poppler 基准、质量档不敏感）。
- **0.6.0 已落地**：`cmyk_conversion` opt-in 开关（设置/CLI `--convert-cmyk`/
  GUI，默认关 = 回到跳过行为；灰度/G4 请求隐式开启；gate 统一拦截
  raw/Indexed/DCT/JPX；分析器按默认关镜像）。
- **0.7.0 主体**：lcms2 真转换——`lcms2` crate（kornelski 安全封装，
  Apache-2.0/MIT，追踪 LCMS 2.19.x；`lcms2-sys` vendored C，同 jpx 的
  feature 门控模式，默认关保持纯 Rust）。无 profile 用内置默认
  CMYK→sRGB，有 ICC 用图像自带 profile；以 poppler/PDFium 渲染对比
  校准目标 profile；渲染比对门禁并入 corpus 快照。转换达标后可评估
  把默认值翻转为开。
- 验收：规范 Adobe YCCK/CMYK 夹具上，压缩前后 poppler 渲染差 ≥25 dB
  （当前 ≈7.6 dB）。

### P1：JBIG2 输入解码（已随 0.6.0 落地）

- `hayro-jbig2` 0.3（Apache-2.0 OR MIT，纯 Rust，T.88 全量，hayro PDF
  渲染器同源）已接入，无 feature 门控；形状门禁、`/JBIG2Globals` 预解析
  管道、poppler 渲染门禁、conformance 语料夹具均已就位（见上文 JBIG2 段）。
- **输出侧维持 JBIG2 门禁结论**（2026-08 备忘录）：G4 仍是唯一双级出口。
- 0.7.0 增量：`/JBIG2Globals` 引用的共享字典在**多图共享一个 globals
  对象**时的去重解码（当前每图各带一份 bytes）；随机接入组织的流内
  形态（embedded 常见，随机接入罕见）。

### P2：JPX 边缘补全（搭车项）

- `/SMask` 为 JPX/DCT 编码的图像（当前 `decode_smask_gray` 只认 raw/flate
  8bit gray，此类图整图跳过）——SMask 解码复用主解码器即可。
- JPX 子采样分量（per-component dx/dy > 1）的上采样支持评估（当前拒绝）。

### P3：大文件与体验

- 2GiB 预检提示已做（0.6.0）；0.7.0 评估上限工程（lopdf 全内存对象图的
  分页/惰性加载属多周工程，收益人群有限——倾向维持上限 + 文档明示）。
- CLI 管道模式（stdin/stdout）与批量目录递归增强（低风险体验项）。

### 维持暂缓

- 线性化（lopdf `SaveOptions::linearize` 空壳，自研多周）；
- JBIG2 输出编码（许可）；简单字体（Type1/PFB、简单 TrueType）子集化（低收益）。

### 节奏建议

0.7.0 主打 CMYK 真转换（lcms2）+ P2 搭车，P3 按余量取舍。
