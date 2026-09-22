# 原生安卓版开发计划（2026-09-20 立项）

> 决策记录：安卓不走 tauri-mobile 变体，**原生 Kotlin + Jetpack Compose 重写
> UI，Rust 引擎（pdf-core）零改动复用**——正确性资产（183+ 测试、变异零
> 漏杀、poppler 校准保真度）100% 平移，重写成本压在最不重要的 UI 层。
> 本计划替代 DEVELOPMENT.md §8「1.x 路线草案」中的 tauri android spike 条目。

## 0. 原则与边界

- **引擎零改动**：pdf-core 不加 Android 特定代码；移动适配全部收在新的
  `pdf-core-ffi` 包装 crate 与 Kotlin 层。引擎现有公共 API（含 0.11.0 定稿
  的字节入口与取消参数）就是移动接口的后端。
- **契约由生成器保证**：UniFFI 从 Rust 接口生成 Kotlin 绑定——与桌面端
  specta/bindings.ts 同一哲学；生成物入库 + CI 新鲜度 diff 门禁（同
  `frontend/src/lib/bindings.ts` 的纪律）。
- **事实源不搬家**：预设表唯一事实源仍在 `CompressionPreset`（Rust），
  安卓经 ffi 读取；错误码文案 = locales en/zh-CN → strings.xml，闭集
  parity 测试同桌面。
- **不进本计划**：iOS（评估另行立项）、桌面端任何改动、引擎功能变更。

## 1. 架构与工件

```
┌─ Kotlin + Jetpack Compose（UI：队列/预设/设置/进度/密码流）─┐
│   ViewModel + StateFlow + WorkManager（后台压缩队列）       │
│   DataStore（设置持久化）· SAF 全程字节 I/O（零存储权限）    │
├─ UniFFI 生成绑定（入库：android/app/src/uniffi/）──────────┤
├─ crates/pdf-core-ffi（workspace 成员，新 crate）            │
│   移动端 DTO + CancelHandle + ProgressCallback + 错误映射   │
├─ pdf-core（零改动；默认特性起步，jpx/cmyk-cms 二期）        │
└─ cargo-ndk → arm64-v8a / x86_64 cdylib ────────────────────┘
```

目录布局（同一仓库）：

| 工件 | 位置 | 说明 |
| --- | --- | --- |
| ffi crate | `crates/pdf-core-ffi/` | workspace 成员；`uniffi` 依赖走 feature 不进引擎 |
| 安卓工程 | `android/` | Gradle Kotlin DSL + Compose；`app` 单模块起步 |
| 绑定生成 | `scripts/gen-android-bindings.sh` | uniffi-bindgen → 生成 Kotlin 入库 |
| 交叉构建 | `scripts/build-android-libs.sh` | cargo-ndk 产 cdylib 到 `android/app/src/main/jniLibs/`（gitignore，构建时产出） |
| CI 腿 | `.github/workflows/android.yml` | 先宿主测试 + APK 组装，真机测试二期 |

## 2. 阶段计划（每阶段含验收钉子）

### Phase 0 —— Spike（2-3 天，风险全部前置）

| 项 | 内容 |
| --- | --- |
| S1 | `rustup target add aarch64-linux-android` + cargo-ndk 交叉编译 pdf-core-ffi 最小壳（仅 `fn ping() -> String`）为 arm64 cdylib |
| S2 | UniFFI proc-macro 接口 → 生成 Kotlin → 空壳 app 调 `ping()` 显示 |
| S3 | 真机/模拟器：`ACTION_OPEN_DOCUMENT` 取 PDF → 字节进 → `compress_pdf_bytes_with_progress` → `ACTION_CREATE_DOCUMENT` 写回，进度条走通、取消生效 |
| S4 | flate2 后端决策验证：移动构建切 `miniz_oxide`（纯 Rust）——zlib-ng 的 cmake/NDK toolchain 配置留作后续优化项，不阻塞 |

**验收**：真机完成一次含图 PDF 压缩（输出能在系统 PDF 阅读器打开）；
进度回调从 Rust 线程到 Compose 状态无 ANR；取消按钮即时生效。
**终止/转向判据**：若 UniFFI 回调线程模型在真机不可用（ANR/JNI 崩溃无法
收敛），降级为手写 JNI + `java.util.function` 风格 C ABI——接口设计不变。

### Phase 1 —— pdf-core-ffi crate（2-3 天）

接口面（Phase 0 壳上长出）：

```rust
#[derive(uniffi::Enum)] pub enum FfiError { MissingInput, InputTooLarge{...},
    PasswordRequired, WrongPassword, EncryptedPdf, Cancelled, InvalidPdf,
    Engine{detail} }          // 11 码的移动镜像（去 Io/Opener/Config 的壳层码）
#[derive(uniffi::Record)] pub struct FfiSettings { ... }     // 镜像 CompressionSettingsPayload
#[derive(uniffi::Record)] pub struct FfiAnalysis { ... } / FfiCompressResult { ... }

#[uniffi::export] fn analyze(bytes: Vec<u8>, password: Option<String>,
    settings: Option<FfiSettings>, progress: Box<dyn FfiProgress>) -> Result<FfiAnalysis, FfiError>;
#[uniffi::export] fn compress(bytes: Vec<u8>, password: Option<String>,
    settings: FfiSettings, cancel: Arc<FfiCancelHandle>,
    progress: Box<dyn FfiProgress>) -> Result<FfiCompressResult, FfiError>;
#[uniffi::export] fn compress_to_target(bytes: Vec<u8>, target_bytes: u64, ...) -> ...;
#[uniffi::export] fn preset_defaults() -> Vec<FfiPresetProfile>;   // 唯一事实源直读
#[uniffi::object] impl FfiCancelHandle { fn cancel(&self); }
```

| 决策 | 取值 |
| --- | --- |
| 移动端输入上限 | ffi 层预检 `MAX_MOBILE_INPUT_BYTES = 512 MiB`（引擎 2 GiB 上限照旧，移动端先友好报错；低内存设备防线） |
| 同步模型 | Rust 暴露阻塞函数 + CancelHandle；Kotlin `withContext(Dispatchers.IO)` + `invokeOnCancellation` 包装（不依赖 UniFFI async） |
| 进度节流 | 引擎 3% 粒度已节流，ffi 不再加层 |
| analyze 取消 | 复用 0.11.0 的 cancel 参数；移动端 analyze 也带 CancelHandle（与 compress 同构） |

**验收**：宿主单测覆盖——DTO 镜像 parity 钉（字段集 = models.rs 的移动
子集，测试钉死）、错误映射全覆盖钉、进度回调顺序钉、取消钉（复用引擎
fixture 套件：noise/零宽/全 CID 等夹具直接跑 ffi 层）、512 MiB 预检钉。

### Phase 2 —— 安卓工程骨架（3-4 天）

- Gradle KTS + Compose BOM + Material3；`minSdk = 26`，`targetSdk` 随当年。
- 主题：跟随系统深浅色；品牌色对齐桌面（复用 tauri 图标资源出
  `ic_launcher` 各密度）。
- i18n：`values/` + `values-zh/`——从 `frontend/src/locales/{en,zh-CN}.ts`
  移植（错误码 11 条 + 压缩/设置文案首版子集）；**parity 测试**：Rust
  错误码闭集 ↔ strings.xml 键闭集（Rust 侧单测读 strings.xml 文件比对，
  同 preset-defaults.json 的 include_str 思路）。
- 构建集成：gradle task 挂 `scripts/build-android-libs.sh`（debug 增量、
  release 走 `cargo-ndk --release` + NDK strip）；绑定再生成 task。
- **验收**：空壳 app 三平台（arm64 真机 / x86_64 模拟器）装配运行；
  `./gradlew assembleDebug` 一条命令从零构建（含 Rust）。

### Phase 3 —— MVP 单文件压缩（1.5-2 周）

功能清单（对齐桌面 UX 范式）：
1. SAF 选文件（`application/pdf` 过滤）→ 字节分析 → 分类/预估/推荐预设
2. 三预设（conservative/balanced/maximum，参数从 `preset_defaults()` 读）
   + 自定义质量/上限简化面板
3. 压缩：进度环（engine 进度映射）+ 取消；结果页：前后大小/节省率/保存
   （`ACTION_CREATE_DOCUMENT` 默认名 `*_optimized.pdf`）+ 分享 sheet
4. 「压不过原件就不写」语义由引擎保证——结果页如实展示 outputWasSmaller
5. 设置持久化：DataStore（预设选择、主题、语言）——**不含桌面 quick
   profile**（右键场景不存在）
6. 密码流：PasswordRequired/WrongPassword → 对话框重试（会话内存，同桌面
   纪律：绝不入 DataStore）

**验收**：真机走查清单（选档/取消/密码/保存/分享/深浅色/中英文各一轮）；
monkey 一轮无崩溃；30 MB 级扫描件压缩内存峰值 < 400 MB（Profiler 记录）。

### Phase 4 —— 队列与后台（约 1 周）

- WorkManager + 前台服务通知：多文件入队（SAF 多选）、逐个压缩、通知
  进度/完成/失败清单；进程被杀可续（WorkManager 重试语义 + 输出写一半
  的临时文件清理——对齐桌面原子写纪律，输出经 SAF 时天然原子于
  DocumentsProvider）。
- 队列 UI：ActivityPanel 范式的列表（状态、逐项进度、重试）。
- **验收**：10 文件队列中途杀进程 → 恢复继续；通知点按回 app；电耗走查
  （Doze 下 WorkManager 退避正常）。

### Phase 5 —— 对齐桌面（1-2 周，可裁剪）

目标大小搜索（`compress_to_target`，预设「5MB 内」chip）；分析详情页
（文档分类/图片统计/预估明细）；最近文件（SAF persistable URI 权限）；
「分享到本应用」入口（`ACTION_SEND` intent-filter 收 PDF）。

### Phase 6 —— 发布工程（2-3 天）

- CI：`android.yml`——ffi 宿主测试 + 绑定新鲜度 diff + 双 ABI 组装 +
  release 签名（secret 起步：debug 公开、release 本地，Play 之前不建 CI
  签名）；lint（detekt 或 android-lint 基线）。
- 版本：`android/app/build.gradle.kts` 的 versionName 对齐仓库版本
  （`sync-version.mjs` 扩展读它；verify-version 断言纳入）。
- 分发：APK 直发（GitHub Releases，随 SHA256SUMS）起步；F-Droid metadata
  草稿（构建说明：Rust + NDK 可复现）；Play 上架另议（账号/费用决策）。
- 文档：USER-GUIDE 安卓节；DEVELOPMENT.md 架构总览补移动层。

## 3. 风险清单

| 风险 | 等级 | 缓解 |
| --- | --- | --- |
| UniFFI 回调跨线程在低端机异常 | 中 | Spike 前置真机验证；降级路径 = 手写 JNI C ABI（接口不变） |
| 大 PDF 内存（解码平面 + 序列化 ≈2× 输入） | 中 | 512 MiB 移动上限 + 引擎 100 MP 预算已有；结果页引导关闭后台 app |
| zlib-ng/cmake × NDK | 中 | 移动构建用 miniz_oxide 后端（决策已定）；zlib-ng 留桌面 |
| UniFFI 版本演进（0.2x 破坏性升级） | 低 | 绑定入库 + 版本锁定在 ffi crate；升级随批次 |
| 引擎 MSRV(1.88) vs NDK 工具链 | 低 | cargo-ndk 只管 target；rustc 版本与桌面同源 |
| APK 体积（cdylib ~8-15 MB） | 低 | 仅 arm64-v8a + x86_64 起步；armeabi-v7a 按需求再议 |
| Kotlin 侧重写工作量 | 已知 | 本计划把它拆进 Phase 2/3/5 的清单里，无隐藏项 |

## 4. 测试策略（延续仓库纪律）

- **宿主层**（CI 必跑）：pdf-core-ffi 单测全量（fixture 复用引擎
  assets——跨平台夹具即输入字节，无障碍）；DTO parity / 错误映射 /
  取消 / 预检四类钉子。
- **组装层**（CI 必跑）：绑定新鲜度 diff；双 ABI `assembleDebug`。
- **真机层**（本地/可选 CI）：Phase 3/4 走查清单固化为
  `androidTest` 冒烟（选档压缩保存一条链路）。
- 变异测试：ffi crate 纳入 mutants 范围（纯逻辑薄层，预计 <20 变异体）。

## 5. 时间线与节奏

| 阶段 | 估时 | 累计 |
| --- | --- | --- |
| P0 Spike | 2-3 天 | ~3 天 |
| P1 ffi crate | 2-3 天 | ~1 周 |
| P2 工程骨架 | 3-4 天 | ~2 周 |
| P3 MVP | 1.5-2 周 | ~3.5 周 |
| P4 队列/后台 | 1 周 | ~4.5 周 |
| P5 桌面对齐 | 1-2 周（可裁剪） | ~6 周 |
| P6 发布工程 | 2-3 天 | ~6.5 周 |

节奏：P0-P2 一个连续批次（出可演示骨架）；P3 一个批次（出可用 MVP，
此时即可发内测 APK）；P4-P6 按 1.0.0 后的 1.x 版本节奏走。与 1.0 的
关系：**互不阻塞**——桌面 1.0 观察期照走，安卓 P0-P2 可并行启动；
`pdf-core-ffi` 合入 main 不触碰冻结面（新增 crate 非 API 变更）。

## 6. 首刀清单（本计划批准后立即执行）

1. `crates/pdf-core-ffi` crate 脚手架 + workspace 接入 + UniFFI 依赖
2. `FfiError`/`FfiSettings`/DTO 首版 + parity/错误映射钉子（宿主测试先行）
3. `ping()` + `compress()` 最小可调路径
4. `scripts/gen-android-bindings.sh` + `build-android-libs.sh`
5. `android/` Gradle 骨架 + `ping()` 上屏
6. 真机 SAF 往返（S3 验收）
