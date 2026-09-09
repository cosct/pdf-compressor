# 测试指南

面向测试贡献者与 QA：本仓库的测试体系、各层怎么跑、质量门禁的判定标准，以及模糊测试/变异测试/真实语料快照的使用方式。架构与工程约定见 [开发指南](DEVELOPMENT.md)；用户视角的功能说明见 [用户指南](USER-GUIDE.zh-CN.md)。

---

## 测试金字塔总览

| 层 | 命令 | 覆盖内容 | 规模 |
| --- | --- | --- | --- |
| 前端单元 | `pnpm test` | 队列/调度/取消（usePdfCompressor）、通知计时、设置面板、右键菜单键盘可达性、App 装配冒烟、en/zh-CN key 树一致性 | 65 项 |
| 引擎单元 | `cargo test -p pdf-core --lib` | 设置夹紧与优先级、分析推荐公式、JPEG 头解析、worker 数量界（含 JPX/CMYK 内存估计）、resize 行为、目标大小搜索调度数学、CCITT 编解码 round-trip、CMYK 减色转换、细节分层 | 92+ 项 |
| 引擎集成 | `cargo test -p pdf-core --lib`（`pdf/tests.rs`） | 真实 lopdf 构造的 PDF 端到端：往返保文本且缩减、去重、SMask、灰度、G3/G4 转码、CMYK（ICC/N=4、DeviceCMYK、CMYK 基 Indexed、部分区间 /Decode 拒绝、feature-off 拒绝彩色转换）、目标大小、加密拒绝/owner 解锁/密码解锁、大纲保留、96 用例变异语料不 panic、不写更大输出、多图目标搜索内存峰值护栏、字节管道（stdin/stdout：压缩胜出/直通/加密分类） | （含于上） |
| 引擎集成（jpx） | `cargo test -p pdf-core --features jpx` | 上面全部 + JPX 码流（J2K 裸流/JP2 容器）转码 JPEG、双级 JPX 转 G4、字典不一致/混合链保持原样、目标大小搜索覆盖新编解码 | （feature 门控） |
| CLI 单元 | `cargo test -p pdf-core --bin pdf-compressor-cli` | 参数解析（`parse_size`/`flag_value`/`split_quick_inputs`、cmyk 开/关旗标）、目录递归展开（大小写扩展名、排序、符号链接防环）、通知文案双语 | 16 项 |
| 桌面壳单元 | `cargo test -p app --lib` | 任务注册表、输出路径白名单、预设配置原子读写 | 7 项 |
| Bindings | `cargo test --workspace` | 由 Rust 签名再生成 `frontend/src/lib/bindings.ts`——**命令签名变更后必须运行并提交** | 1 项 |
| 基准 | `cargo bench -p pdf-core` | 全管线各预设、编码器对比（jpeg-encoder vs image crate） | criterion |
| 模糊测试 | `cargo +nightly fuzz run pipeline` / `fuzz run jpx` | 任意字节跑 analyze+compress+目标大小搜索；jpx target 专打 OpenJPEG C 解码路径 | CI 每次 push 各 45s 冒烟 |
| 变异测试 | `cargo mutants` | 引擎测试对实现的杀伤力 | 本地按需（无 CI 作业） |

## 一键全量

```bash
pnpm test                          # 前端
cargo test --workspace            # Rust 全部 + bindings 再生成
cargo clippy --workspace --all-targets -- -D warnings
pnpm run check                     # 前端格式化 + lint + 类型检查
```

CI（`.github/workflows/ci.yml`）在每次 push/PR 执行以上全部，外加双 MSRV 检查（引擎 1.88 / 壳 1.93，用各自声明的工具链 `cargo check --locked`）与依赖审计（cargo audit + pnpm audit）。

---

## 质量门禁（防“能压缩但变糊”）

### 提交内的 PSNR 门禁

`preset_quality_is_monotonic_and_above_floors` 用 1200×900 噪声夹具（PSNR 最坏情形：逐像素独立噪声在 JPEG 量化下失真最大）逐预设比对解码输出与源平面：

| 预设 | 下限 | 实测（校准值） |
| --- | --- | --- |
| conservative | ≥ 28 dB | ≈ 30.1 dB |
| balanced | ≥ 25.5 dB | ≈ 27.4 dB |
| maximum | ≥ 22.5 dB | ≈ 24.3 dB |

并要求三档单调（conservative ≥ balanced ≥ maximum）。阈值钉在实测下约 2dB——掉过线即说明某次改动让“同一个质量号”变得明显更差。

### 真实语料快照（opt-in）

合成夹具无法代表真实文档。把一批真实 PDF 放进一个目录，然后：

```bash
PDF_COMPRESSOR_QUALITY_CORPUS=/path/to/corpus cargo test -p pdf-core --lib real_corpus
```

- **首跑**：逐文件 × 三预设压缩，记录体积比（输出/输入）与平均图片 PSNR，写 `<目录>/quality-baseline.json`
- **后续跑**：对基线回归告警——某预设体积比增长 >5%（压得不如以前狠）或平均 PSNR 下降 >1dB（画质回归）即测试失败
- 未设环境变量时该测试 no-op，CI 保持离线确定性

建议语料构成：文本型（论文/合同）、混合型（带照片的报告）、扫描件（灰度/G3/G4 传真）、含 ICC 的印刷稿、加密件各若干。语料不入库。

---

## 模糊测试

```bash
cd crates/pdf-core
cargo +nightly fuzz run pipeline -- -max_total_time=45      # CI 同款冒烟
cargo +nightly fuzz run pipeline -- -max_total_time=600     # 本地长跑
cargo +nightly fuzz run jpx -- -max_total_time=600          # JPX C 解码路径长跑
```

`pipeline` 目标把任意输入字节写盘后依次跑 analyze → compress（含字体子集化——内嵌字体程序解析器与图片编解码同属对抗性输入面）→ target-size 搜索。不变量：**任何输入都不 panic、不挂起**；损坏图片/字体只产生 skip。

`jpx` 目标（feature `jpx` 已在 fuzz crate 强制启用）把任意字节包成最小合法 PDF 的 JPXDecode 流、并填充到小流跳过阈值之上，保证每次迭代都进入 vendored OpenJPEG 的 C 解码——这是 unsafe 面的专职硬化（通用 pipeline 目标无法从随机字节演化出带 JPX 流的合法 PDF）。本地 3 分钟 ≈ 35k 次迭代无崩溃为基线。崩溃现场在 `fuzz/artifacts/`，用 `cargo +nightly fuzz fmt` 还原最小用例后加进 `pdf/tests.rs` 的变异语料测试。

## 变异测试

本地按需工具（无 CI 作业——hosted runner 上全量套件需 5h+，超过 GitHub 6h 作业硬上限，2026-09-08 实测每变异体约 110s）。推荐按改动范围收窄：

```bash
git diff $(git merge-base origin/main HEAD) > wip.diff
cargo mutants -p pdf-core --in-diff wip.diff    # 只变异本次改动的代码
cd crates/pdf-core && cargo mutants            # 本地全量（数小时）
```

报告在 `mutants.out/`。存活变异（survived mutant）= 测试盲区，修复优先级：纯函数（设置夹紧、搜索调度、编解码）> 编排逻辑 > 交互层。

---

## 写新测试的约定

- **夹具**：用 `crates/pdf-core/src/testutil.rs` 的确定性生成器（`fixture_rgb_image`/`gradient_rgb_image`/`bilevel_scan_image`/`encode_jpeg`/`encode_ccitt_g4`/`encode_ccitt_g3_1d`/`luma_psnr_db`），不要在测试里内联复制生成逻辑
- **CCITT 形状**：G3/G4 输入夹具的 `DecodeParms` 必须与编码器旗标一致（`encode_ccitt_g3_1d(image, byte_align, with_eol)` 对应字典里的 `EncodedByteAlign`/`EndOfLine`），形状门禁在 `stream_filter_info` 收口
- **JPX 夹具**：committed 码流在 `crates/pdf-core/assets/jpx-*.j2k/.jp2`（lossless，经 `scripts/make-jpx-fixtures.sh` 用 opj_compress 再生），参考平面是 testutil 的 `jpx_rgb_reference`/`jpx_gray_reference`/`jpx_bilevel_reference`——改图案必须两边同步并重新生成资产。JPX 相关测试一律 `#[cfg(feature = "jpx")]` 门控
- **JBIG2 夹具**：`assets/jbig2-scan.bin`（conformance 语料真实 A4 扫描页的 embedded 形态，`scripts/make-jbig2-fixture.sh` 再生并打印页尺寸）——换样本必须同步 `jbig2_scan_dimensions()`；无许可可用的编码器派生独立像素真值，保真锚点是 poppler 渲染对比门禁（pdftoppm 缺失时跳过）；JBIG2 无 feature 门控，测试不加 cfg
- **CFF 字体夹具**：`assets/test-font-cid.cff/.otf`（Source Han Serif CN 的 CID 键控子集，非恒等 charset，`scripts/make-cff-fixture.sh` 再生）——改字形集必须连同 `TEST_CFF_CID_*` 常量一起更新；CFF 子集化测试有 pdftoppm 渲染比对（poppler 缺失时自动跳过）
- **加密夹具**：`encrypt_fixture(path, owner, user)` 用 lopdf 标准 handler（V1/RC4）；空 user 密码 = owner-only 件
- **集成测试模式**：`build_pdf_bytes*` 构造 → 写临时目录 → 跑引擎 → `Document::load` 重载断言。断言“输出仍是合法 PDF + 文本保留”是每个改写类测试的底线
- **前端**：新增 locale key 必须同时进 `en.ts` 与 `zh-CN.ts`（`locales.spec.ts` 强制 key 树一致）；改命令签名后跑 `cargo test --workspace` 再生成 bindings 并提交
- **目标大小搜索**：涉及搜索调度的改动跑 `target_size_search_produces_reproducible_output`（确定性）与调度数学的单元测试

## 发布前检查单

1. `pnpm run sync-version` 版本三处对齐（README 已不再携带版本号；**CHANGELOG 新版本段落必须就位**——打 tag 后 release workflow 会用该段落自动生成 release notes，缺失则 `release-notes` job 直接失败，杜绝空 body 发版）
2. 上面“一键全量”全绿
3. 真实语料快照跑一轮（若有语料）
4. AUR PKGBUILD 按_release workflow_ 的 `aur-checksum` job 输出更新 sha256

发版前可本地预览生成的 notes：`./scripts/release-notes.sh v<版本>`（打印到 stdout，与 job 产出一致）。
