# 0.11.0 开发计划：审查收口专项（2026-09-18 立项）

> 个人项目单版本收口：不切 patch/minor 列车，本专项全部落地为一个版本。
> 完成后按 DEVELOPMENT.md §8 的 1.0 触发条件重新计时观察期。

## 0. 背景与证据基线

- **来源**：2026-09-17/18 两份独立全面审查（A：引擎/壳与前端/工程化三方向；
  B：七方向红队视角），随后对 B 报告的 41 条论断逐项核验——**38 条属实**
  （其中 6 条亲手复现）、**2 条推翻**、1 条报告作者自证伪（Form 递归栈溢出，
  10 万/30 万层链对照实验 exit 0，认可其更正）。
- **已复现的崩溃证据**（夹具已生成，收编见 P4-3）：
  - 零宽图片：652 B PDF（`/Width 0 /Height 10 /DeviceGray`）→ `compress`
    panic 于 `encode.rs:1766`（`chunks(0)`），exit 101，无输出文件；
  - 字体 remap 溢出：293 KB PDF（Type0/Identity-H + 全 65536 CID）→
    `--preset maximum` panic 于 subsetter-0.2.6 `remapper.rs:91`，exit 101；
  - target-size 三连：`--target-size 3400K` 报 `quality: "255"`；
    管道+target-size 输出 507,574 B 却报 `outputWasSmaller:false`；
    `--target-size 200K` 仅用预算 41%（质量钉死地板 15）。
- **被推翻、不得进入本计划的两条**：①"新 clone `cargo test --workspace`
  必因 frontend/dist 失败"——tauri-codegen 2.6.3 在 dev 模式跳过该检查，
  裸测试可跑（`mkdir` 占位仅 `--all-features` 需要）；② dispatch 输入插值
  的坐标——真实注入面在 `aur-publish.yml:47`，非 ci/release.yml。

## 1. 主题与范围

**主题：红队视角收口**——默认配置下 KB 级输入不崩不炸、target-size 说
真话、前端状态机无竞态、发布链每步有断言。

**明确不进本版本**：OS 级代码签名（证书是外部条件，见 §7 暂缓）、安卓、
TypeScript 7、DEVELOPMENT.md §8 既有暂缓项（线性化/JBIG2 输出/2GiB 流式/
PDF/A 等，结论全部维持）。

## 2. P0：输入健壮性（7 项，本版本硬门槛）

共用模式：**分配前预算 + 既有 skip 通道**，一人连续做完保持口径一致。

- **2.1 零/负宽度图片防护**：`decode_raw_image_stream`（encode.rs:1159）
  与 Indexed 分支入口拒绝 `width < 1 || height < 1`（含 Real 截断为 0、
  负值）；两个 CCITT 出口前加 `plane.width() > 0` 兜底；照抄
  `decode_smask_gray`（encode.rs:836-843）已有写法。
  钉子：零宽/负宽/`Width 0.5` 三夹具走 skip 而非 panic。
- **2.2 subsetter remap 溢出**：fonts.rs:188 循环前
  `if old_gids.len() > 1 + u16::MAX as usize { continue; }`，并按字体真实
  numGlyphs 过滤越界 GID。钉子：65536-CID 夹具转为跳过子集化、任务完成。
- **2.3 解压上限全覆盖**：mod.rs:103 LoadOptions 设
  `max_decompressed_size`；13 处无界解压点（encode.rs:913/1163、
  fonts.rs:100/182/337/412/788、colorspace.rs:268/307、compressor.rs:941、
  resources.rs:156/531、analyzer.rs:788）换 lopdf `*_with_limit` 孪生 API；
  预算 = 声明几何尺寸 + 每文档累计上限；超限走 skip 通道。
  钉子：高压缩比流夹具被跳过且整任务完成。
- **2.4 像素总量预算**：CCITT/JBIG2/raw 分配前校验 `w×h ≤ 预算`
  （建议 100 MP），替代"上限即分配值"（现状 65535²=4.29 GB）；
  `try_reserve_exact` 失败映射 skip。钉子：65535×65535 声明的 1 KB
  CCITT 流不再触发 GB 级分配。
- **2.5 JPEG 信任链对齐**：JPEG 解码后校验码流 SOF vs 字典维度（对齐
  jpx.rs:109 / jbig2.rs:162 已有一致性校验）；zune 路径
  （cmyk.rs:243-255）`DecoderOptions` 设显式上限（现状默认 16384²×4≈1 GiB/
  平面，绕过 image crate 的 512 MiB）。钉子：字典 100×100 / 码流巨图
  夹具被拒绝或安全跳过。
- **2.6 worker panic 隔离**：workers.rs `run_worker_pool` 内
  `catch_unwind` 包裹任务，panic 转任务级错误（现 `thread::scope` 会把
  已构造的 AppError 换成整体 panic）；compressor.rs:1524
  `join().unwrap_or(0)` 至少记日志（现静默吞 panic 当 0 字节成功）。
  钉子：注入 panic 的测试任务不影响整任务且有日志。
- **2.7 畸形 /Filter 收口**：`stream_filter_info`（encode.rs:1898-1913）
  对非 Name/Array 的 `/Filter`（如 `/Filter 42`）判不可解码 → skip
  （现 lopdf `filters()` 报错回退 `content.clone()`，压缩字节被当像素，
  产出"成功"的垃圾图）。钉子：`/Filter 42` 夹具走 skip。

## 3. P1：功能正确性（5 项）

- **3.1 quality 255 越界**：target_size.rs:314 `hi` 初始值 `u8::MAX` →
  100；:493-496 clamp 上限同步；修 doc 注释矛盾。
  钉子：`--target-size 3400K` 报告 quality ≤ 100。
- **3.2 管道+target-size 账目**：target_size.rs:456-478 字节分支补齐
  `outputWasSmaller/savedBytes/savingsPercent`，与文件分支一致。
  钉子：管道模式实测输出更小时三字段如实。
- **3.3 搜索质量钉死**：target_size.rs:336-345 缩边后允许在新边长下重新
  上探质量（`hi = collapse_quality` 的单向收敛改为区间重置，或显式文档化
  保守语义）；同步更新 `nothing_fitting_keeps_the_range_falling_until_the_floor`
  的预期。钉子：200K 预算用例利用率显著高于 41%（或 CHANGELOG 声明取舍）。
- **3.4 输出原子写**：compressor.rs:508 改"同目录临时文件 + sync_all +
  rename"（复用 quick_profile.rs:70-98 纪律）；rename 前最后一次取消
  检查；`OutputClaimGuard` 能清理非空失败产物（现只清 0 字节）。
  钉子：ENOSPC/中途 kill 不留截断 PDF。
- **3.5 死错误变体**：`AppError::Encrypted` 接入 DRM 检测路径（DRM 文件
  现只报"密码错误"，用户无限重试）或删除变体；`AppError::Image` 同理。
  钉子：错误码 taxonomy 双侧钉同步；DRM 夹具提示可区分。

## 4. P2：前端状态机与契约（6 项，建议同一轮改完）

四条互相纠缠（取消状态机 ↔ 密码弹窗 ↔ 队列恢复 ↔ 预设契约）。

- **4.1 取消/双击竞态**：usePdfCompressor.ts —— `runCompressionTargets`
  在 `await analysisQueuePromise`（:667）之后重查 `compressionRunning`；
  `compressJob` 入口（:569）检查 `cancelledCompressionRuns.has(runId)`；
  取消完成前禁止新 run（或 run 代际令牌）。
  钉子：新增"取消→立即重启""分析中双击"两个回归测试（现有测试只覆盖
  迟到结果丢弃）。
- **4.2 密码链路**：去 trim（tauri.ts:122/179、usePdfCompressor.ts:714，
  仅空串归 null——现含空格合法密码被静默破坏）；App.vue:189-213 的
  dismiss 按"错误实例"去重而非 job.id（现注释声称"新失败重开"但无复位
  代码，同 job 本会话不再弹）。
  钉子：含空格密码解锁用例；dismiss 后再次失败重弹用例。
- **4.3 预设/配置契约**：presets.ts:76-79 sanitizer 接受 `'jpeg'`（现保存/
  加载双向静默改写为 g4，与队列侧 usePdfCompressor.ts:109 口径矛盾）；
  CONFIG_VERSION 单源化（现 presets.ts:28=2 / models.rs=3 / tauri.ts:258=??3
  三处，GUI 写的 v2 会被后端 v<3 迁移再剥一次显式 jpeg）；恢复队列先写回
  `job.settings` 再触发分析（:902-910 顺序颠倒，现分析用 draft 设置）。
  钉子：jpeg 预设保存/加载不漂移；v3 文件不被重复迁移；恢复队列预估基于
  正确设置。
- **4.4 冷启动 argv**（审查 A 发现）：lib.rs setup 解析 `std::env::args()`，
  复用 single-instance 的 `.pdf` 过滤；前端 listener 就绪前一次性缓存
  重放；macOS 补 `Opened` 事件（现应用未运行时双击 PDF 不入队）。
  钉子：冷启动带参用例（手动验收 + 代码路径单测）。
- **4.5 splash 超时兜底**（审查 A 发现）：Rust 侧 ~10s 未收 `app_ready`
  自动关 splash 显主窗（现前端 mount 失败永久卡加载页）。
- **4.6 同步命令搬家（可选）**：`save_preset_user_config`（同步 sync_all）/
  `reveal_path_in_folder`（阻塞 D-Bus）/ `existing_paths`（逐路径 exists）
  改 async 或 spawn_blocking（commands.rs:261/329/583；14 命令仅 3 个
  async）。

## 5. P3：发布链与供应链（8 项）

- **5.1 版本一致性前置 job**：release.yml 第一步断言 tag == package.json
  == 两个 Cargo.toml == Cargo.lock == CHANGELOG 段落存在；
  `sync-version.mjs` 改走 `cargo metadata` 并同步 lock；Arch 模板 0.7.1
  改占位符。验收：打错 tag 第一分钟失败。
- **5.2 Windows 图标 + 装后冒烟**（审查 A 发现，已复核）：context-menu.nsh:16
  Icon `$INSTDIR\pdf-compressor.exe,0` → `$INSTDIR\app.exe,0`（未设
  mainBinaryName，安装后实为 app.exe）；release Windows 腿加解包 NSIS
  校验 exe/资源路径的冒烟。验收：右键子菜单图标不再空白。
- **5.3 AUR 渲染加固**：aur-publish.yml sed 后断言 pkgver/sha256 已替换
  （现 `sha256sums=('SKIP')`/旧版本可静默上位）；加 `makepkg
  --verifysource`；**:47 输入插值改经 env 传参**（真实注入面）；先渲染
  校验再依次 push。
- **5.4 cmake 承诺对齐**：libz-ng-sys 1.1.29 `build = "zng/cmake.rs"` 强制
  cmake 无 cc 回退，与 Cargo.toml:86-88 / DEVELOPMENT.md:243-245 的
  "纯 Rust、不需要 cmake" 相悖，且 PKGBUILD.source makedepends 缺 cmake
  （净 chroot 必挂）。三选一：makedepends 加 cmake（最小改动）/ flate2
  换后端 / 保留现状但修正注释与文档。验收：AUR 源码包净 chroot 构建通过；
  文档不再撒谎。
- **5.5 OpenJPEG 升级**：vendored 2.5.3（opj_config.h）→ ≥2.5.4
  （CVE-2025-54874 修复版，适用性升级前确认）；升不动则评估发布产物关
  `jpx`（jpx 现随三平台 release 与 AUR 启用）。
- **5.6 签名与校验**：本版本只做诚实面——README/USER-GUIDE 明示未签名
  状态与放行方法；release 附 SHA256SUMS（aur-publish 已在算，上传零
  成本）。OS 级签名见 §7 暂缓。
- **5.7 CI 加固**：第三方 action 全部钉 SHA（dependabot 已配置可续更）；
  ci.yml 顶层 `permissions: contents: read` + `timeout-minutes`；audit
  加 weekly schedule 并覆盖 fuzz 独立锁（现漂移 24 个 crate 无人管）；
  release 构建统一 `--locked`（现仅 CLI 腿用）；测试矩阵加
  windows-latest/macos-latest 核心腿（现全 ubuntu，5.2 类缺陷只有平台
  验证能拦）。
- **5.8 门禁机制化**：CI 加 `cargo test export_bindings` + `git diff
  --exit-code frontend/src/lib/bindings.ts`（现零漂移靠运气）；质量门与
  CI 的 `--locked`/`--frozen-lockfile` 对齐；poppler 缺失时 CI 显式失败、
  本地醒目汇总（现画质门禁静默 skip 后照样打印"全部通过"）；Rust 侧
  `include_str!` preset-defaults.json 让"逐值镜像"名副其实（现测试硬编码
  字面量，改 JSON 全绿），或修正声明。

## 6. P4：文档 / 合规 / 仓库卫生（批量，可穿插）

- **6.1 文档修正批量**：TESTING.md PSNR 表对齐代码实际下限
  （25.5/23.0/20.5，文档写下限的其实是旧实测值）与失效 `aur-checksum`
  引用、两处计数漂移；DEVELOPMENT.md 命令数统一为 14（现 13/15 两处）；
  CHANGELOG 补 0.6.0+ 链接定义并更新 `[未发布]` 锚。
- **6.2 许可合规**：新增 THIRD-PARTY-NOTICES（jpeg-encoder 的 IJG、
  vendored OpenJPEG BSD-2、lcms2 MIT、夹具字体 OFL）；AUR `license` 字段
  同步；`make-cff-fixture.sh` 的 `--name-IDs` 保留 OFL 许可文本（13/14）
  或在夹具目录放许可副本（现 Source Han Serif 衍生夹具剥了许可文本）。
- **6.3 复现夹具收编**：本次核验用的零宽/负宽 PDF、65536-CID 字体 PDF、
  noise 图 PDF 转为 `scripts/make-*.sh` 再生脚本（遵循现有
  make-cff/jbig2/jpx-fixture 惯例），进 `crates/pdf-core/assets/` 并被
  2.1/2.2/3.1-3.3 的钉子测试引用；补 jpx-indexed.jp2（1.2 MB，现存唯一
  无配方夹具）的再生记录。
- **6.4 卫生**：scripts 统一 `chmod +x`（现仅 release-notes.sh 是 755）；
  .gitignore 加 `src-tauri/binaries/`（release 流程写入大二进制）；
  mutants-diff.sh:20 改 `command -v cargo-mutants`；`tempfile` 移
  `[dev-dependencies]`；pdf-core 的 bilevelCodec 过期 doc comment（仍写
  jpeg 默认）更新并重生成 bindings；清理 `.audit-tmp/`。
- **6.5 测试基建**：fuzz 加 `compress_pdf_bytes_with_progress` 字节入口
  target（现 2 个 target 全走文件路径，公共字节 API 零覆盖；DCT/ICC/
  SMask/CCITT 语料为空）；变异测试产物落 `docs/mutation-log.md` 或 CI
  artifact（现"0 漏"无入库产物可复核，CHANGELOG 口径与产物对不上）。
- **6.6 可选重构**（同域顺手，不单独立项）：encode.rs 的 CCITT 编解码
  （~330 行）独立成 `ccitt.rs`；`PdfBuild(String)` 拆分引擎 bug 与输入
  错误；`PlaneSink::fill` 加 `take==0` 防御（现依赖 hayro-jbig2 回调契约
  才不可达）；analyzer 的 skip policy 镜像 grayscale（现预估偏保守）；
  Form 递归加显式深度上限（栈溢出已被实验证伪，此项防病态输入的无界
  遍历成本，纯加固）。
- **6.7 CMYK Adobe 反转核查（先调查后定论）**：cmyk.rs:37-40 与 :224-230
  注释互相矛盾，全 crate 无 APP14 解析，唯一反转来自显式 /Decode。先造
  "APP14 transform-0 且无 /Decode"夹具与 poppler 渲染对齐，再决定是否
  补解析——未做像素级验证前不改转换逻辑（静默反色是最坏失败模式，
  但误改同样）。

## 7. 本专项新增暂缓

- **OS 级代码签名（Apple 公证 / Windows Authenticode）**：证书与年费是
  外部条件；本版本只做 5.6 的诚实声明 + SHA256SUMS。解锁条件：决定长期
  维护发布渠道并购置证书。
- 其余暂缓项维持 DEVELOPMENT.md §8 结论不变。

## 8. 验收总表

在 0.10.0 基线（引擎默认/特性双门禁、CLI 19、e2e 11+1、壳 12、前端 67）
之上：

1. `pnpm gate` 全绿 + CI 全绿（含新增 windows/macos 腿与钉 SHA 后的
   action 更新）；
2. P0 七项钉子：三个崩溃夹具转为 skip/友好错误（exit ≠ 101），DoS 夹具
   内存有界；
3. P1 五项钉子：target-size 三用例输出如实（quality ≤ 100、管道账目
   一致、预算利用率达标）；原子写故障注入不留残文件；
4. P2 钉子：前端新增竞态/密码/预设回归测试全绿， bindings 零漂移由 CI
   diff 拦截；
5. P3：版本一致性 job 演示一次"打错 tag 第一分钟失败"；AUR 渲染断言
   演示一次占位值拦截；
6. CHANGELOG 0.11.0 条目：按面分组（健壮性/正确性/前端契约/发布链/
   合规），每条附 file:line 与钉子测试名——延续 0.9.0 诚实性体例。

## 9. 执行顺序（个人串行）

P0（2.1→2.7 连续做，共用预算/skip 口径）→ P1（3.1-3.3 同文件连续，
3.4/3.5 随后）→ P2（4.1-4.3 同一轮，4.4/4.5 独立小块）→ P3（5.4/5.5
涉及构建定义先行，5.1-5.3 发布链集中改）→ P4 穿插收尾。每完成一项：
钉子测试 + `pnpm gate`，禁止批量攒到最后过门。

---

## 10. 落地状态（2026-09-18 实施后复核）

**已完成**：P0 全部 7 项（两条可复现崩溃以原始夹具复验 exit 0）；P1 全部
5 项（target-size 三连以 3400K/500K 管道/200K 三用例复验）；P2 的 4.1-4.5；
P3 全部 8 项；P4 除下述项。质量门全绿，新增约 27 个钉子测试。

**与计划的偏差（均已复核接受）**：

- 3.3 的根因比外部报告更深：质量钉死地板源于 u8::MAX 起点浪费前两次探
  测，而非仅缩边重启策略；最终修法为"诚实上界 + 质量盲探测坍缩"（宽重
  启单独使用会破坏 JBIG2 多级缩边的探测经济性，已被其钉子测试拦截）。
- 2.3 的严格解压上限一度破坏合法尾部填充流（image from_raw 为"至少"语
  义）——已加 25%+64 KiB 宽容并钉测试。
- 6.3 夹具采用 encode.rs 内联合成 helper 而非 assets/ + 再生脚本——可复
  现性以测试代码承载；二进制夹具的 make-*.sh 方案仍未覆盖。

**未完成 / 遗留**：

- **6.7 CMYK APP14 调查**：未启动（计划本身要求先造夹具与 poppler 对齐
  再决定）。cmyk.rs:37-40 与 :224-230 的注释矛盾仍在，留待独立调查——
  在此之前不动转换逻辑（误改与漏改同险）。
- 4.6 同步命令迁移（计划标可选）：未做。
- jpx-indexed.jp2（1.2 MB）再生配方：来源无从考证，无法伪造脚本。
- 变异测试：产物已落 docs/mutation-log.md（0.11.0 收口批次的全量
  campaign 待发布前补跑）。

**第三方复核驱动的补丁（2026-09-18 第二轮）**：

- verify-version 初版两处必然失败（根 Cargo.toml 为虚拟清单、awk 残留空
  格）——已重写并以 v0.10.0 本地复刻验证通过。
- 写盘路径补齐：写前最后一次取消检查（save_and_build_response* 透传
  cancel_flag）；临时文件改 create_new + pid 后缀 + 重试（防同名覆盖与
  符号链接）；OutputClaimGuard 兜底清扫残留 `<output>.<pid>.tmp`。
  并补自动化钉子：create_new 防覆盖（哨兵文件存活）、残留清扫精确性
  （无关文件不动）、空占位移除三个用例（compressor.rs tests）。
- AUR license 字段同步为四元组，THIRD-PARTY-NOTICES.md 随源码包与 zst
  分发。
- CHANGELOG 增加 [未发布] 节（含 error.image 移除的破坏性声明与
  512 MiB loader 上限的作用面说明——该上限仅作用于加载期解码的对象/
  xref 流，普通内容/图片流不受影响）。
